//! MPV session lifecycle without Tauri: launch, IPC tracking, resume history and
//! Discord refresh. A shell supplies paths, settings and UI hooks via [`MpvHost`].

use crate::discord_presence::{update_discord_presence, DiscordPresence};
use crate::mpv::*;
use crate::player_settings::{validate_mpv_path, PlayerSettings};
use crate::shikimori::{presence_title_id, ShikimoriCache};
use discord_rich_presence::DiscordIpcClient;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

/// What a shell provides to the MPV session logic. Cheap to clone (a handle).
pub trait MpvHost: Clone + Send + Sync + 'static {
    fn mpv(&self) -> &MpvState;
    fn discord(&self) -> &Mutex<Option<DiscordIpcClient>>;
    fn shikimori(&self) -> &ShikimoriCache;
    fn user_agent(&self) -> &'static str;
    /// `https://<site host>` of the page that asked for playback.
    fn site_origin(&self) -> String;
    fn player_settings(&self) -> Result<PlayerSettings, String>;
    fn logs_dir(&self) -> Result<PathBuf, String>;
    fn history_path(&self) -> Result<PathBuf, String>;
    /// Short toolbar/page status such as "⚠ MPV".
    fn report_status(&self, text: &str, color: &str, reset_after_ms: u64, detail: Option<String>);
}

#[derive(Default)]
pub struct MpvState {
    pub current: Mutex<Option<MpvSession>>,
    pub embedded: Mutex<DiscordPresence>,
    pub history: Mutex<()>,
    pub progress_owners: Mutex<std::collections::BTreeMap<String, String>>,
    pub rpc_pending: std::sync::atomic::AtomicBool,
    pub launch: tokio::sync::Mutex<()>,
}

impl MpvState {
    pub fn owns_progress(&self, sample: &MpvSession) -> bool {
        self.progress_owners
            .lock()
            .is_ok_and(|owners| owners.get(&sample.media_key) == Some(&sample.session_id))
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MpvLaunchInfo {
    pub session_id: String,
    pub mirror_embedded: bool,
}

pub struct MpvLaunchRequest {
    pub url: String,
    pub title: Option<String>,
    pub start_time: Option<f64>,
    pub sub_url: Option<String>,
    pub token: Option<String>,
    pub media: Option<PlaybackMedia>,
}

/// Blocking: reads the history file.
pub fn playback_resume(host: &impl MpvHost, key: &str) -> Result<Option<ResumePoint>, String> {
    let _guard = host.mpv().history.lock().map_err(|e| e.to_string())?;
    Ok(read_history(&host.history_path()?)?.resume(key))
}

/// Blocking: rewrites the history file.
pub fn clear_playback_history(host: &impl MpvHost) -> Result<(), String> {
    let _guard = host.mpv().history.lock().map_err(|e| e.to_string())?;
    write_history(&host.history_path()?, &PlaybackHistory::default())
}

pub fn persist_mpv_progress(host: &impl MpvHost, sample: &MpvSession) -> Result<(), String> {
    let state = host.mpv();
    let owners = state.progress_owners.lock().map_err(|e| e.to_string())?;
    if owners.get(&sample.media_key) != Some(&sample.session_id) || !sample.playback.ready {
        return Ok(());
    }
    if let Some(position) = sample.playback.position {
        if valid_media_key(&sample.media_key) {
            let state = host.mpv();
            let _guard = state.history.lock().map_err(|e| e.to_string())?;
            let path = host.history_path()?;
            let mut history = read_history(&path)?;
            history.record(&sample.media_key, position, sample.playback.duration);
            write_history(&path, &history)?;
        }
    }
    Ok(())
}
pub fn refresh_mpv_discord<H: MpvHost>(host: &H) {
    use std::sync::atomic::Ordering;
    if host.mpv().rpc_pending.swap(true, Ordering::AcqRel) {
        return;
    }
    let host = host.clone();
    tokio::task::spawn_blocking(move || {
        let state = host.mpv();
        if let Ok(mut client) = host.discord().lock() {
            let sample = state.current.lock().ok().and_then(|s| s.clone());
            let presence = sample
                .as_ref()
                .filter(|s| s.playback.ready)
                .map(mpv_presence)
                .unwrap_or_else(|| state.embedded.lock().map(|p| p.clone()).unwrap_or_default());
            let mut presence = presence;
            host.shikimori().decorate(&mut presence);
            let _ = update_discord_presence(&mut client, &presence);
        }
        state.rpc_pending.store(false, Ordering::Release);
    });
}

pub async fn track_mpv<H: MpvHost>(
    host: H,
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
                    Ok(0)=>{
                        // mpv closes the socket just before it exits; give it a moment
                        // so a normal quit is not reported as a lost connection.
                        match tokio::time::timeout(std::time::Duration::from_secs(2), child.wait()).await {
                            Ok(Ok(status)) if status.success() => {}
                            Ok(_) => failure=Some("MPV завершился с ошибкой. Проверьте журнал плеера.".to_string()),
                            Err(_) => failure=Some("Соединение с MPV закрыто".to_string()),
                        }
                        break;
                    },
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
                    let state=host.mpv();
                    let mut current=state.current.lock().unwrap_or_else(|e|e.into_inner());
                    if current.as_ref().is_some_and(|s|s.session_id==sample.session_id){*current=Some(sample.clone());true}else{false}
                };
                if owned {refresh_mpv_discord(&host);}
                if persisted.elapsed().as_secs()>=5 && host.mpv().owns_progress(&sample) {
                    if let Err(error)=persist_mpv_progress(&host,&sample){failure=Some(format!("Не удалось сохранить позицию: {error}"));}
                    persisted=std::time::Instant::now();
                }
                if !sample.playback.ready && started.elapsed().as_secs()>60 {failure=Some("MPV не подтвердил открытие видео за 60 секунд".into());break;}
            }
        }
    }
    let _ = persist_mpv_progress(&host, &sample);
    let owned = {
        let state = host.mpv();
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
        refresh_mpv_discord(&host);
        if let Some(error) = failure {
            host.report_status("⚠ MPV", "#ff5252", 10000, Some(error));
        }
    }
    // Closing Anivox never kills a user's external player. Reap our child independently.
    if child.try_wait().ok().flatten().is_none() {
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
    }
}

pub async fn open_in_mpv<H: MpvHost>(
    host: H,
    request: MpvLaunchRequest,
) -> Result<MpvLaunchInfo, String> {
    let MpvLaunchRequest {
        url,
        title,
        start_time,
        sub_url,
        token,
        media,
    } = request;
    let state = host.mpv();
    let _launch = state.launch.try_lock().map_err(|_| "MPV уже запускается")?;
    let site_origin = host.site_origin();
    let host_copy = host.clone();
    let (command, session_id, mirror_embedded, ipc_dir, endpoint) =
        tokio::task::spawn_blocking(move || {
            let settings = host_copy.player_settings()?;
            let settings = PlayerSettings {
                mpv_path: validate_mpv_path(settings.mpv_path)?,
                ..settings
            };
            let logs = host_copy.logs_dir()?;
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
        tokio::spawn(async move {
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
        let host = host.clone();
        tokio::spawn(async move {
            host.shikimori().ensure(id, host.user_agent()).await;
            refresh_mpv_discord(&host);
        });
    }
    tokio::spawn(track_mpv(host.clone(), child, stream, sample, ipc_dir));
    Ok(MpvLaunchInfo {
        session_id,
        mirror_embedded,
    })
}
