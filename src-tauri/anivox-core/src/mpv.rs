//! MPV launch arguments, IPC protocol, playback state and resume history.
//! No Tauri types: the shell decides where files live and how events are shown.

use crate::discord_presence::DiscordPresence;
use crate::player_settings::PlayerSettings;
use std::path::Path;
use std::process::Command;

pub fn build_mpv_command(
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
pub struct PlaybackMedia {
    pub key: String,
    pub presence: DiscordPresence,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumePoint {
    pub position: f64,
    pub duration: Option<f64>,
    pub updated_at: u64,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct PlaybackHistory {
    pub entries: std::collections::BTreeMap<String, ResumePoint>,
}

pub fn valid_media_key(key: &str) -> bool {
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
    pub fn record(&mut self, key: &str, position: f64, duration: Option<f64>) {
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
    pub fn resume(&self, key: &str) -> Option<ResumePoint> {
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

pub fn read_history(path: &Path) -> Result<PlaybackHistory, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| format!("Не удалось прочитать историю: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(PlaybackHistory::default()),
        Err(e) => Err(e.to_string()),
    }
}
pub fn write_history(path: &Path, history: &PlaybackHistory) -> Result<(), String> {
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
pub struct MpvPlayback {
    pub ready: bool,
    pub paused: bool,
    pub buffering: bool,
    pub ended: bool,
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub speed: f64,
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
    pub fn apply(&mut self, event: &serde_json::Value) {
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
pub struct MpvSession {
    pub session_id: String,
    pub media_key: String,
    pub mirror_embedded: bool,
    #[serde(flatten)]
    pub playback: MpvPlayback,
    #[serde(skip)]
    pub media: Option<PlaybackMedia>,
}
pub fn mpv_presence(sample: &MpvSession) -> DiscordPresence {
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
pub trait MpvIo: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> MpvIo for T {}
pub async fn connect_mpv_ipc(endpoint: &str) -> std::io::Result<Box<dyn MpvIo>> {
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
pub async fn observe_mpv(stream: &mut Box<dyn MpvIo>) -> Result<(), String> {
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
