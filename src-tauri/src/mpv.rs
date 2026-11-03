use crate::discord::{
    decorate_presence, ensure_shikimori_info, presence_title_id, update_discord_presence,
    DiscordPresence, DiscordState,
};
use crate::ensure_local_window;
use crate::player_settings::{
    cached_player_settings, player_logs_path, validate_mpv_path, PlayerSettings,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use tauri::Manager;

pub(crate) fn build_mpv_command(
    settings: &PlayerSettings,
    log_path: &Path,
    url: String,
    title: Option<String>,
    start_time: Option<f64>,
    sub_url: Option<String>,
    token: Option<String>,
    site_origin: &str,
) -> Command {
    let mut cmd = Command::new(settings.mpv_path.as_deref().unwrap_or("mpv"));

    if let Some(t) = title {
        if !t.is_empty() {
            cmd.arg(format!("--force-media-title={}", t));
        }
    }

    if let Some(st) = start_time {
        if st > 0.0 {
            cmd.arg(format!("--start={:.2}", st));
        }
    }

    if let Some(sub) = sub_url {
        if !sub.is_empty() {
            cmd.arg(format!("--sub-file={}", sub));
        }
    }

    // Pass headers cleanly to MPV and FFmpeg demuxer
    cmd.arg(format!("--referrer={site_origin}/"));
    cmd.arg(format!("--http-header-fields-append=Origin: {site_origin}"));
    if let Some(tok) = token {
        if !tok.is_empty() {
            cmd.arg(format!(
                "--http-header-fields-append=Authorization: Bearer {}",
                tok
            ));
        }
    }
    cmd.arg("--user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0");

    // UI and logging flags: show window immediately, don't exit silently on error, write debug log
    cmd.arg("--force-window=immediate");
    cmd.arg("--keep-open=yes");
    cmd.arg("--hls-bitrate=max");
    cmd.arg("--ytdl-format=bestvideo+bestaudio/best");
    let mut log_arg = std::ffi::OsString::from("--log-file=");
    log_arg.push(log_path);
    cmd.arg(log_arg);
    cmd.arg("--").arg(url);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaybackMedia {
    pub(crate) key: String,
    pub(crate) presence: DiscordPresence,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResumePoint {
    pub(crate) position: f64,
    pub(crate) duration: Option<f64>,
    pub(crate) updated_at: u64,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct PlaybackHistory {
    pub(crate) entries: std::collections::BTreeMap<String, ResumePoint>,
}

pub(crate) fn valid_media_key(key: &str) -> bool {
    let parts: Vec<_> = key.split(':').collect();
    key.len() <= 80
        && parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|c| c.is_ascii_digit())
                && part.parse::<u64>().is_ok_and(|n| n > 0)
        })
}

impl PlaybackHistory {
    pub(crate) fn record(&mut self, key: &str, position: f64, duration: Option<f64>) {
        if !valid_media_key(key)
            || !position.is_finite()
            || !(0.0..=7.0 * 86400.0).contains(&position)
        {
            return;
        }
        let duration = duration.filter(|n| n.is_finite() && *n > 0.0 && *n <= 7.0 * 86400.0);
        let updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.entries.insert(
            key.into(),
            ResumePoint {
                position,
                duration,
                updated_at,
            },
        );
        while self.entries.len() > 1000 {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, p)| p.updated_at)
                .map(|(key, _)| key.clone());
            if let Some(key) = oldest {
                self.entries.remove(&key);
            }
        }
    }
    pub(crate) fn resume(&self, key: &str) -> Option<ResumePoint> {
        if !valid_media_key(key) {
            return None;
        }
        self.entries
            .get(key)
            .filter(|p| {
                p.position.is_finite()
                    && p.position >= 10.0
                    && p.position <= 7.0 * 86400.0
                    && p.duration.is_none_or(|d| {
                        d.is_finite() && d > 0.0 && p.position < d * 0.95 && d - p.position > 30.0
                    })
            })
            .cloned()
    }
}

pub(crate) fn history_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("playback-history.json"))
}
pub(crate) fn read_history(path: &Path) -> Result<PlaybackHistory, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| format!("Не удалось прочитать историю: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(PlaybackHistory::default()),
        Err(e) => Err(e.to_string()),
    }
}
pub(crate) fn write_history(path: &Path, history: &PlaybackHistory) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().ok_or("Не найден каталог истории")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(history).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.error.to_string())?;
    Ok(())
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MpvPlayback {
    pub(crate) ready: bool,
    pub(crate) paused: bool,
    pub(crate) buffering: bool,
    pub(crate) ended: bool,
    pub(crate) position: Option<f64>,
    pub(crate) duration: Option<f64>,
    pub(crate) speed: f64,
}
impl Default for MpvPlayback {
    fn default() -> Self {
        Self {
            ready: false,
            paused: true,
            buffering: false,
            ended: false,
            position: None,
            duration: None,
            speed: 1.0,
        }
    }
}
impl MpvPlayback {
    pub(crate) fn apply(&mut self, event: &serde_json::Value) {
        match event["event"].as_str() {
            Some("file-loaded") => {
                self.ready = true;
                self.ended = false;
            }
            Some("end-file") => {
                self.ended = true;
                self.paused = true;
            }
            Some("property-change") => {
                let data = &event["data"];
                match event["name"].as_str() {
                    Some("pause") => {
                        if let Some(v) = data.as_bool() {
                            self.paused = v;
                        }
                    }
                    Some("paused-for-cache") => {
                        if let Some(v) = data.as_bool() {
                            self.buffering = v;
                        }
                    }
                    Some("eof-reached") => {
                        if let Some(ended) = data.as_bool() {
                            self.ended = ended;
                            if ended {
                                self.paused = true;
                            }
                        }
                    }
                    Some("time-pos") => {
                        if let Some(v) = data.as_f64().filter(|v| v.is_finite() && *v >= 0.0) {
                            self.position = Some(v);
                        }
                    }
                    Some("duration") => {
                        if let Some(v) = data.as_f64().filter(|v| v.is_finite() && *v > 0.0) {
                            self.duration = Some(v);
                        }
                    }
                    Some("speed") => {
                        if let Some(v) = data
                            .as_f64()
                            .filter(|v| v.is_finite() && *v > 0.0 && *v <= 16.0)
                        {
                            self.speed = v;
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    }
}
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MpvSession {
    pub(crate) session_id: String,
    pub(crate) media_key: String,
    pub(crate) mirror_embedded: bool,
    #[serde(flatten)]
    pub(crate) playback: MpvPlayback,
    #[serde(skip)]
    pub(crate) media: Option<PlaybackMedia>,
}
#[derive(Default)]
pub(crate) struct MpvState {
    pub(crate) current: Mutex<Option<MpvSession>>,
    pub(crate) embedded: Mutex<DiscordPresence>,
    pub(crate) history: Mutex<()>,
    pub(crate) progress_owners: Mutex<std::collections::BTreeMap<String, String>>,
    pub(crate) rpc_pending: std::sync::atomic::AtomicBool,
    pub(crate) launch: tokio::sync::Mutex<()>,
}
impl MpvState {
    pub(crate) fn owns_progress(&self, sample: &MpvSession) -> bool {
        self.progress_owners
            .lock()
            .is_ok_and(|owners| owners.get(&sample.media_key) == Some(&sample.session_id))
    }
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MpvLaunchInfo {
    pub(crate) session_id: String,
    pub(crate) mirror_embedded: bool,
}

pub(crate) fn ensure_content_access(webview: &tauri::Webview) -> Result<(), String> {
    if webview.label() == "content" {
        Ok(())
    } else {
        Err("Доступно только в плеере Anivox".into())
    }
}
#[tauri::command]
pub fn get_mpv_state(
    app: tauri::AppHandle,
    webview: tauri::Webview,
) -> Result<Option<MpvSession>, String> {
    ensure_content_access(&webview)?;
    let value = app
        .state::<MpvState>()
        .current
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    Ok(value)
}
#[tauri::command]
pub async fn get_playback_resume(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    key: String,
) -> Result<Option<ResumePoint>, String> {
    ensure_content_access(&webview)?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<MpvState>();
        let _guard = state.history.lock().map_err(|e| e.to_string())?;
        Ok(read_history(&history_path(&app)?)?.resume(&key))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn clear_playback_history(
    app: tauri::AppHandle,
    webview: tauri::Webview,
) -> Result<(), String> {
    ensure_local_window(&webview, "settings")?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<MpvState>();
        let _guard = state.history.lock().map_err(|e| e.to_string())?;
        write_history(&history_path(&app)?, &PlaybackHistory::default())
    })
    .await
    .map_err(|e| e.to_string())?
}
pub(crate) fn persist_mpv_progress(
    app: &tauri::AppHandle,
    sample: &MpvSession,
) -> Result<(), String> {
    let state = app.state::<MpvState>();
    let owners = state.progress_owners.lock().map_err(|e| e.to_string())?;
    if owners.get(&sample.media_key) != Some(&sample.session_id) || !sample.playback.ready {
        return Ok(());
    }
    if let Some(position) = sample.playback.position {
        if valid_media_key(&sample.media_key) {
            let state = app.state::<MpvState>();
            let _guard = state.history.lock().map_err(|e| e.to_string())?;
            let path = history_path(app)?;
            let mut history = read_history(&path)?;
            history.record(&sample.media_key, position, sample.playback.duration);
            write_history(&path, &history)?;
        }
    }
    Ok(())
}
pub(crate) fn mpv_presence(sample: &MpvSession) -> DiscordPresence {
    let mut presence = sample
        .media
        .as_ref()
        .map(|m| m.presence.clone())
        .unwrap_or_default();
    if presence.details.is_empty() {
        presence.details = "Anivox".into();
    }
    let status = if sample.playback.ended {
        "Просмотр завершён"
    } else if sample.playback.buffering {
        "Загрузка"
    } else if sample.playback.paused {
        "На паузе"
    } else {
        "Смотрит"
    };
    presence.state_text = format!(
        "{}{}MPV · {status}",
        presence.state_text,
        if presence.state_text.is_empty() {
            ""
        } else {
            " · "
        }
    );
    presence.state_text = presence.state_text.chars().take(128).collect();
    presence.end_timestamp = None;
    if !sample.playback.paused && !sample.playback.buffering && !sample.playback.ended {
        if let (Some(position), Some(duration)) =
            (sample.playback.position, sample.playback.duration)
        {
            let remaining = (duration - position) / sample.playback.speed;
            if remaining > 0.0 && remaining < 7.0 * 86400.0 {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                presence.end_timestamp = Some(now as i64 + remaining.ceil() as i64);
            }
        }
    }
    presence
}
pub(crate) fn refresh_mpv_discord(app: &tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    if app
        .state::<MpvState>()
        .rpc_pending
        .swap(true, Ordering::AcqRel)
    {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<MpvState>();
        if let Ok(mut client) = app.state::<DiscordState>().0.lock() {
            let sample = state.current.lock().ok().and_then(|s| s.clone());
            let presence = sample
                .as_ref()
                .filter(|s| s.playback.ready)
                .map(mpv_presence)
                .unwrap_or_else(|| state.embedded.lock().map(|p| p.clone()).unwrap_or_default());
            let mut presence = presence;
            decorate_presence(&app, &mut presence);
            let _ = update_discord_presence(&mut client, &presence);
        }
        state.rpc_pending.store(false, Ordering::Release);
    });
}

pub(crate) trait MpvIo: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> MpvIo for T {}
pub(crate) async fn connect_mpv_ipc(endpoint: &str) -> std::io::Result<Box<dyn MpvIo>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(
            tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint)?,
        ))
    }
    #[cfg(unix)]
    {
        Ok(Box::new(tokio::net::UnixStream::connect(endpoint).await?))
    }
    #[cfg(not(any(target_os = "windows", unix)))]
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "MPV IPC unsupported",
        ))
    }
}
pub(crate) async fn observe_mpv(stream: &mut Box<dyn MpvIo>) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    for (id, property) in [
        "pause",
        "time-pos",
        "duration",
        "speed",
        "paused-for-cache",
        "eof-reached",
    ]
    .iter()
    .enumerate()
    {
        let command = serde_json::json!({"command":["observe_property",id+1,property]});
        stream
            .write_all(format!("{command}\n").as_bytes())
            .await
            .map_err(|e| e.to_string())?;
    }
    // Query time-pos too: file-loaded may precede the IPC connection.
    stream
        .write_all(b"{\"command\":[\"get_property\",\"time-pos\"],\"request_id\":100}\n")
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub(crate) async fn track_mpv(
    app: tauri::AppHandle,
    mut child: tokio::process::Child,
    stream: Box<dyn MpvIo>,
    mut sample: MpvSession,
    _ipc_dir: tempfile::TempDir,
) {
    use tokio::io::AsyncBufReadExt;
    let mut reader = tokio::io::BufReader::new(stream);
    let mut line = Vec::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut persisted = std::time::Instant::now();
    let started = std::time::Instant::now();
    let mut failure = None;
    loop {
        tokio::select! {
            result=reader.read_until(b'\n',&mut line)=>{
                match result {
                    Ok(0)=>{if child.try_wait().ok().flatten().is_none(){failure=Some("Соединение с MPV закрыто".to_string());}break;},
                    Ok(_) if line.len()<=65536=>{
                        if let Ok(event)=serde_json::from_slice::<serde_json::Value>(&line) {
                            sample.playback.apply(&event);
                            if event["request_id"]==100 && event["error"]=="success" && event["data"].as_f64().is_some_and(|n|n.is_finite() && n>=0.0) {sample.playback.ready=true;}
                            if sample.playback.ended && event["reason"]=="error" {failure=Some("MPV не смог открыть видео. Проверьте журнал плеера.".into());}
                        }
                        line.clear();
                    },
                    _=>{failure=Some("Ошибка чтения состояния MPV".into());break;}
                }
            },
            result=child.wait()=>{
                if !result.is_ok_and(|s|s.success()){failure=Some("MPV завершился с ошибкой. Проверьте журнал плеера.".into());}
                break;
            },
            _=tick.tick()=>{
                let owned={
                    let state=app.state::<MpvState>();
                    let mut current=state.current.lock().unwrap_or_else(|e|e.into_inner());
                    if current.as_ref().is_some_and(|s|s.session_id==sample.session_id){*current=Some(sample.clone());true}else{false}
                };
                if owned {refresh_mpv_discord(&app);}
                if persisted.elapsed().as_secs()>=5 && app.state::<MpvState>().owns_progress(&sample) {
                    if let Err(error)=persist_mpv_progress(&app,&sample){failure=Some(format!("Не удалось сохранить позицию: {error}"));}
                    persisted=std::time::Instant::now();
                }
                if !sample.playback.ready && started.elapsed().as_secs()>60 {failure=Some("MPV не подтвердил открытие видео за 60 секунд".into());break;}
            }
        }
    }
    let _ = persist_mpv_progress(&app, &sample);
    let owned = {
        let state = app.state::<MpvState>();
        let mut current = state.current.lock().unwrap_or_else(|e| e.into_inner());
        if current
            .as_ref()
            .is_some_and(|s| s.session_id == sample.session_id)
        {
            *current = None;
            true
        } else {
            false
        }
    };
    if owned {
        refresh_mpv_discord(&app);
        if let Some(error) = failure {
            let _ = crate::report_player_status(
                app.clone(),
                "⚠ MPV".into(),
                "#ff5252".into(),
                10000,
                Some(error),
            );
        }
    }
    // Closing Anivox never kills a user's external player. Reap our child independently.
    if child.try_wait().ok().flatten().is_none() {
        tauri::async_runtime::spawn(async move {
            let _ = child.wait().await;
        });
    }
}

#[tauri::command]
pub async fn open_in_mpv(
    app: tauri::AppHandle,
    url: String,
    title: Option<String>,
    start_time: Option<f64>,
    sub_url: Option<String>,
    token: Option<String>,
    media: Option<PlaybackMedia>,
) -> Result<MpvLaunchInfo, String> {
    let state = app.state::<MpvState>();
    let _launch = state.launch.try_lock().map_err(|_| "MPV уже запускается")?;
    let site_origin = crate::site_hosts::site_origin_for(
        app.get_webview("content")
            .and_then(|view| view.url().ok())
            .as_ref(),
    );
    let app_copy = app.clone();
    let (command, session_id, mirror_embedded, ipc_dir, endpoint) =
        tauri::async_runtime::spawn_blocking(move || {
            let settings = cached_player_settings(&app_copy)?;
            let settings = PlayerSettings {
                mpv_path: validate_mpv_path(settings.mpv_path)?,
                ..settings
            };
            let logs = player_logs_path(&app_copy)?;
            std::fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let session_id = format!("{}-{stamp}", std::process::id());
            let ipc_dir = tempfile::Builder::new()
                .prefix("anivox-mpv-")
                .tempdir()
                .map_err(|e| e.to_string())?;
            #[cfg(target_os = "windows")]
            let endpoint = format!(r"\\.\pipe\anivox-mpv-{session_id}");
            #[cfg(not(target_os = "windows"))]
            let endpoint = ipc_dir.path().join("ipc").to_string_lossy().into_owned();
            let mut command = build_mpv_command(
                &settings,
                &logs.join(format!("mpv-{session_id}.log")),
                url,
                title,
                start_time,
                sub_url,
                token,
                &site_origin,
            );
            // Insert before the option terminator; URLs always remain positional arguments.
            let args: Vec<_> = command.get_args().map(|s| s.to_os_string()).collect();
            let mut ipc_command = Command::new(command.get_program());
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                ipc_command.creation_flags(0x08000000);
            }
            ipc_command.arg(format!("--input-ipc-server={endpoint}"));
            ipc_command.args(args);
            command = ipc_command;
            Ok::<_, String>((
                command,
                session_id,
                settings.mirror_embedded,
                ipc_dir,
                endpoint,
            ))
        })
        .await
        .map_err(|e| e.to_string())??;
    let mut child = tokio::process::Command::from(command)
        .spawn()
        .map_err(|e| format!("Не удалось запустить MPV: {e}. Выберите плеер в настройках."))?;
    let mut connected = None;
    for _ in 0..80 {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return Err("MPV завершился до подключения IPC. Проверьте журнал плеера.".into());
        }
        match connect_mpv_ipc(&endpoint).await {
            Ok(stream) => {
                connected = Some(stream);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
        }
    }
    let Some(mut stream) = connected else {
        tauri::async_runtime::spawn(async move {
            let _ = child.wait().await;
        });
        return Err("MPV запущен, но IPC недоступен. Встроенный плеер не изменён.".into());
    };
    observe_mpv(&mut stream).await?;
    let media = media.map(|mut m| {
        m.key = if valid_media_key(&m.key) {
            m.key
        } else {
            String::new()
        };
        m.presence.details = m.presence.details.chars().take(128).collect();
        m.presence.state_text = m.presence.state_text.chars().take(80).collect();
        m
    });
    let sample = MpvSession {
        session_id: session_id.clone(),
        media_key: media.as_ref().map(|m| m.key.clone()).unwrap_or_default(),
        mirror_embedded,
        playback: MpvPlayback::default(),
        media,
    };
    if valid_media_key(&sample.media_key) {
        let mut owners = state.progress_owners.lock().map_err(|e| e.to_string())?;
        owners.insert(sample.media_key.clone(), sample.session_id.clone());
        while owners.len() > 1000 {
            let key = owners.keys().find(|key| *key != &sample.media_key).cloned();
            if let Some(key) = key {
                owners.remove(&key);
            } else {
                break;
            }
        }
    }
    let title = sample
        .media
        .as_ref()
        .and_then(|m| presence_title_id(&m.presence));
    *state.current.lock().map_err(|e| e.to_string())? = Some(sample.clone());
    if let Some(id) = title {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            ensure_shikimori_info(&app, id).await;
            refresh_mpv_discord(&app);
        });
    }
    tauri::async_runtime::spawn(track_mpv(app.clone(), child, stream, sample, ipc_dir));
    Ok(MpvLaunchInfo {
        session_id,
        mirror_embedded,
    })
}
