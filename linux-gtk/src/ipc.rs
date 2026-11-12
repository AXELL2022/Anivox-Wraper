//! Command dispatcher behind the WebKit message handler. Commands are blocking
//! and must be called off the GTK main thread; anything that touches widgets
//! is sent to the main thread as a [`UiEvent`].

use crate::connection::SegmentStats;
use anivox_core::discord_presence::{update_discord_presence, DiscordPresence};
use anivox_core::mpv::{PlaybackMedia, ResumePoint};
use anivox_core::mpv_session::{self as session, MpvHost, MpvLaunchRequest, MpvState};
use anivox_core::player_settings::{load_player_settings, PlayerSettings};
use anivox_core::shikimori::{presence_title_id, ShikimoriCache};
use anivox_core::site_hosts;
use discord_rich_presence::DiscordIpcClient;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use url::Url;

const USER_AGENT: &str = concat!("Anivox/", env!("CARGO_PKG_VERSION"));

/// Requests for the GTK main thread.
#[derive(Debug)]
pub enum UiEvent {
    Status {
        text: String,
        color: String,
        reset_after_ms: u64,
        detail: Option<String>,
    },
    Fullscreen(bool),
}

struct Shared {
    mpv: MpvState,
    discord: Mutex<Option<DiscordIpcClient>>,
    shikimori: ShikimoriCache,
    config_dir: PathBuf,
    data_dir: PathBuf,
    /// Current page address; set by the main thread, used for the Referer origin.
    page: Mutex<Option<String>>,
    ui: async_channel::Sender<UiEvent>,
    /// Latest video segment summary from `segment-monitor.js`.
    segments: Mutex<Option<SegmentStats>>,
}

#[derive(Clone)]
pub struct GtkHost(Arc<Shared>);

impl GtkHost {
    pub fn new(ui: async_channel::Sender<UiEvent>, config_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self(Arc::new(Shared {
            mpv: MpvState::default(),
            discord: Mutex::new(None),
            shikimori: ShikimoriCache::default(),
            config_dir,
            data_dir,
            page: Mutex::new(None),
            ui,
            segments: Mutex::new(None),
        }))
    }

    pub fn set_page(&self, uri: Option<String>) {
        if let Ok(mut page) = self.0.page.lock() {
            *page = uri;
        }
    }

    fn send(&self, event: UiEvent) {
        let _ = self.0.ui.send_blocking(event);
    }
}

impl MpvHost for GtkHost {
    fn mpv(&self) -> &MpvState {
        &self.0.mpv
    }
    fn discord(&self) -> &Mutex<Option<DiscordIpcClient>> {
        &self.0.discord
    }
    fn shikimori(&self) -> &ShikimoriCache {
        &self.0.shikimori
    }
    fn user_agent(&self) -> &'static str {
        USER_AGENT
    }
    fn site_origin(&self) -> String {
        let page = self.0.page.lock().ok().and_then(|page| page.clone());
        site_hosts::site_origin_for(page.and_then(|uri| Url::parse(&uri).ok()).as_ref())
    }
    fn player_settings(&self) -> Result<PlayerSettings, String> {
        load_player_settings(&self.0.config_dir.join("settings.json"))
    }
    fn logs_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.data_dir.join("logs"))
    }
    fn history_path(&self) -> Result<PathBuf, String> {
        Ok(self.0.config_dir.join("playback-history.json"))
    }
    fn report_status(&self, text: &str, color: &str, reset_after_ms: u64, detail: Option<String>) {
        self.send(UiEvent::Status {
            text: text.into(),
            color: color.into(),
            reset_after_ms,
            detail,
        });
    }
}

pub struct Ipc {
    host: GtkHost,
    runtime: tokio::runtime::Runtime,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenInMpv {
    url: String,
    title: Option<String>,
    start_time: Option<f64>,
    sub_url: Option<String>,
    token: Option<String>,
    media: Option<PlaybackMedia>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayerStatus {
    text: String,
    color: String,
    #[serde(default)]
    reset_after_ms: u64,
    detail: Option<String>,
}

impl Ipc {
    pub fn new(host: GtkHost) -> Result<Self, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|err| err.to_string())?;
        Ok(Self { host, runtime })
    }

    pub fn host(&self) -> &GtkHost {
        &self.host
    }

    /// Runs a future on the background runtime; call off the GTK main thread.
    pub fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }

    /// For the native connection panel.
    pub fn segment_stats(&self) -> Option<SegmentStats> {
        self.host.0.segments.lock().ok()?.clone()
    }

    /// Used by the native settings panel only; remote pages have no such command.
    pub fn clear_playback_history(&self) -> Result<(), String> {
        session::clear_playback_history(&self.host)
    }

    pub fn handle(&self, command: &str, args: Value) -> Result<Value, String> {
        match command {
            "set_discord_rpc" => self.set_discord_rpc(args),
            "get_mpv_state" => to_json(self.host.mpv().current.lock().map_err(err)?.clone()),
            "get_playback_resume" => {
                let key = args["key"].as_str().ok_or("Нет ключа серии")?;
                let point: Option<ResumePoint> = session::playback_resume(&self.host, key)?;
                to_json(point)
            }
            "open_in_mpv" => {
                let request: OpenInMpv = from_json(args)?;
                let launched = self.runtime.block_on(session::open_in_mpv(
                    self.host.clone(),
                    MpvLaunchRequest {
                        url: request.url,
                        title: request.title,
                        start_time: request.start_time,
                        sub_url: request.sub_url,
                        token: request.token,
                        media: request.media,
                    },
                ))?;
                to_json(launched)
            }
            "report_player_status" => {
                let status: PlayerStatus = from_json(args)?;
                self.host.send(UiEvent::Status {
                    text: status.text,
                    color: status.color,
                    reset_after_ms: status.reset_after_ms,
                    detail: status.detail,
                });
                Ok(Value::Null)
            }
            "report_segment_stats" => {
                let stats: SegmentStats =
                    from_json(args.get("stats").cloned().unwrap_or(Value::Null))?;
                *self.host.0.segments.lock().map_err(err)? = Some(stats.sanitized());
                Ok(Value::Null)
            }
            "content_fullscreen" => {
                let fullscreen = args["fullscreen"].as_bool().unwrap_or(false);
                self.host.send(UiEvent::Fullscreen(fullscreen));
                Ok(Value::Null)
            }
            other => Err(format!("Команда недоступна в этой оболочке: {other}")),
        }
    }

    fn set_discord_rpc(&self, args: Value) -> Result<Value, String> {
        let mut presence: DiscordPresence =
            from_json(args.get("presence").cloned().unwrap_or(Value::Null))?;
        presence.truncate_texts();
        if let Some(id) = presence_title_id(&presence) {
            self.runtime
                .block_on(self.host.shikimori().ensure(id, USER_AGENT));
        }
        let mut client = self.host.discord().lock().map_err(err)?;
        // The embedded player keeps its last status; an active MPV session owns Discord.
        let mpv = self.host.mpv();
        *mpv.embedded.lock().map_err(err)? = presence.clone();
        if mpv
            .current
            .lock()
            .map_err(err)?
            .as_ref()
            .is_some_and(|session| session.playback.ready)
        {
            return Ok(Value::Null);
        }
        self.host.shikimori().decorate(&mut presence);
        update_discord_presence(&mut client, &presence)?;
        Ok(Value::Null)
    }
}

fn err<E: ToString>(error: E) -> String {
    error.to_string()
}

fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(err)
}

fn to_json<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ipc(dir: &std::path::Path) -> (Ipc, async_channel::Receiver<UiEvent>) {
        let (tx, rx) = async_channel::unbounded();
        let host = GtkHost::new(tx, dir.join("config"), dir.join("data"));
        (Ipc::new(host).unwrap(), rx)
    }

    #[test]
    fn unknown_commands_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, _rx) = ipc(dir.path());
        let err = ipc.handle("close_shikimori", json!({})).unwrap_err();
        assert!(err.contains("close_shikimori"), "{err}");
    }

    #[test]
    fn malformed_presence_is_rejected_before_touching_discord() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, _rx) = ipc(dir.path());
        assert!(ipc
            .handle("set_discord_rpc", json!({"presence": 5}))
            .is_err());
        assert!(ipc.host().discord().lock().unwrap().is_none());
    }

    #[test]
    fn presence_without_discord_running_reports_an_error_and_can_retry() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, _rx) = ipc(dir.path());
        let args = json!({"presence": {"details": "Тест", "stateText": "Серия 1"}});
        // Point the socket search at an empty directory: no Discord there. TMPDIR is left
        // alone because other tests create temporary directories concurrently.
        std::env::set_var("XDG_RUNTIME_DIR", dir.path());
        assert!(ipc.handle("set_discord_rpc", args.clone()).is_err());
        assert!(ipc.handle("set_discord_rpc", args).is_err());
        // The embedded status is remembered even when Discord is unavailable.
        assert_eq!(ipc.host().mpv().embedded.lock().unwrap().details, "Тест");
    }

    #[test]
    fn mpv_state_is_empty_and_resume_needs_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, _rx) = ipc(dir.path());
        assert_eq!(ipc.handle("get_mpv_state", json!({})).unwrap(), Value::Null);
        assert!(ipc.handle("get_playback_resume", json!({})).is_err());
        let none = ipc
            .handle("get_playback_resume", json!({"key": "1:2:3"}))
            .unwrap();
        assert_eq!(none, Value::Null);
    }

    #[test]
    fn status_and_fullscreen_are_forwarded_to_the_ui() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, rx) = ipc(dir.path());
        ipc.handle(
            "report_player_status",
            json!({"text": "MPV", "color": "#fff", "resetAfterMs": 5, "detail": null}),
        )
        .unwrap();
        ipc.handle("content_fullscreen", json!({"fullscreen": true}))
            .unwrap();
        assert!(matches!(rx.try_recv().unwrap(), UiEvent::Status { .. }));
        assert!(matches!(rx.try_recv().unwrap(), UiEvent::Fullscreen(true)));
    }

    #[test]
    fn segment_stats_are_stored_for_the_connection_panel() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, _rx) = ipc(dir.path());
        assert!(ipc.segment_stats().is_none());
        assert!(ipc
            .handle("report_segment_stats", json!({"stats": 1}))
            .is_err());
        ipc.handle(
            "report_segment_stats",
            json!({"stats": {"host": "cdn", "active": true, "mbps": 12.5, "updatedAt": 5.0}}),
        )
        .unwrap();
        let stats = ipc.segment_stats().unwrap();
        assert_eq!((stats.host.as_str(), stats.mbps), ("cdn", 12.5));
    }

    #[test]
    fn launching_a_missing_player_fails_with_a_clear_message() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, _rx) = ipc(dir.path());
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        let settings = json!({"mpvPath": "/definitely/missing/mpv"});
        std::fs::write(
            dir.path().join("config/settings.json"),
            settings.to_string(),
        )
        .unwrap();
        let err = ipc
            .handle("open_in_mpv", json!({"url": "https://example.test/v.m3u8"}))
            .unwrap_err();
        assert!(err.contains("MPV"), "{err}");
    }
}

#[cfg(all(test, unix))]
mod mpv_flow_tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    /// A stand-in for mpv: serves the JSON IPC socket, reports a loaded file at
    /// 12.5 s of 1400 s, records its argv and exits after about six seconds.
    const FAKE_MPV: &str = r#"#!/usr/bin/env python3
import json, os, socket, sys, time
argv = sys.argv[1:]
open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "argv.txt"), "w").write("\n".join(argv))
path = [a.split("=", 1)[1] for a in argv if a.startswith("--input-ipc-server=")][0]
srv = socket.socket(socket.AF_UNIX); srv.bind(path); srv.listen(1)
conn, _ = srv.accept()
def send(obj): conn.sendall((json.dumps(obj) + "\n").encode())
conn.settimeout(0.2)
buf = b""
end = time.time() + 6
sent = False
while time.time() < end:
    try:
        chunk = conn.recv(4096)
        if not chunk: break
        buf += chunk
    except socket.timeout:
        pass
    while b"\n" in buf:
        line, buf = buf.split(b"\n", 1)
        cmd = json.loads(line)
        if cmd["command"][0] == "get_property":
            send({"request_id": cmd.get("request_id"), "error": "success", "data": 12.5})
    if not sent:
        sent = True
        send({"event": "file-loaded"})
        for name, data in (("time-pos", 12.5), ("duration", 1400.0), ("pause", False)):
            send({"event": "property-change", "name": name, "data": data})
conn.close()
time.sleep(0.5)
sys.exit(0)
"#;

    #[test]
    fn mpv_session_launches_tracks_and_saves_progress() {
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            return; // no interpreter for the stand-in player
        }
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("fake-mpv");
        std::fs::write(&fake, FAKE_MPV).unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("settings.json"),
            json!({"mpvPath": fake.to_string_lossy()}).to_string(),
        )
        .unwrap();

        let (tx, rx) = async_channel::unbounded();
        let host = GtkHost::new(tx, config, dir.path().join("data"));
        host.set_page(Some("https://anivox.me/anime/1-x".into()));
        let ipc = Ipc::new(host).unwrap();

        let launched = ipc
            .handle(
                "open_in_mpv",
                json!({
                    "url": "https://cdn.example.test/v.m3u8",
                    "title": "Серия 3",
                    "startTime": 42.5,
                    "token": "tok",
                    "media": {"key": "1:2:3", "presence": {"details": "Аниме"}}
                }),
            )
            .unwrap();
        let session_id = launched["sessionId"].as_str().unwrap().to_owned();

        std::thread::sleep(std::time::Duration::from_millis(2500));
        let state = ipc.handle("get_mpv_state", json!({})).unwrap();
        assert_eq!(state["sessionId"], session_id);
        assert_eq!(state["ready"], true);
        assert_eq!(state["paused"], false);
        assert_eq!(state["position"], 12.5);
        assert_eq!(state["duration"], 1400.0);

        // The fake exits by itself: the session ends and progress is stored.
        std::thread::sleep(std::time::Duration::from_secs(5));
        assert_eq!(ipc.handle("get_mpv_state", json!({})).unwrap(), Value::Null);
        let resume = ipc
            .handle("get_playback_resume", json!({"key": "1:2:3"}))
            .unwrap();
        assert_eq!(resume["position"], 12.5);
        assert_eq!(resume["duration"], 1400.0);

        // A normal exit must not raise the "⚠ MPV" warning.
        assert!(
            rx.try_recv().is_err(),
            "unexpected status after a clean exit"
        );

        let argv = std::fs::read_to_string(dir.path().join("argv.txt")).unwrap();
        for expected in [
            "--force-media-title=Серия 3",
            "--start=42.50",
            "--referrer=https://anivox.me/",
            "--http-header-fields-append=Origin: https://anivox.me",
            "--http-header-fields-append=Authorization: Bearer tok",
        ] {
            assert!(
                argv.lines().any(|arg| arg == expected),
                "{expected}\n{argv}"
            );
        }
        assert_eq!(argv.lines().last(), Some("https://cdn.example.test/v.m3u8"));
    }
}

#[cfg(all(test, unix))]
mod real_mpv_tests {
    use super::*;
    use serde_json::json;

    /// Needs `mpv`, `ffmpeg`, `pkill` and a display (X11/Wayland):
    /// `cargo test -- --ignored real_mpv`.
    #[test]
    #[ignore = "needs a real mpv, ffmpeg and a display"]
    fn real_mpv_reports_playback_and_saves_progress() {
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("test.mp4");
        let made = std::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x180:rate=25",
            ])
            .args(["-f", "lavfi", "-i", "sine=frequency=440", "-t", "120"])
            .args(["-pix_fmt", "yuv420p", "-shortest"])
            .arg(&video)
            .status()
            .unwrap();
        assert!(made.success());

        let (tx, rx) = async_channel::unbounded();
        let host = GtkHost::new(tx, dir.path().join("config"), dir.path().join("data"));
        host.set_page(Some("https://anivox.fun/anime/1-x".into()));
        let ipc = Ipc::new(host).unwrap();

        let launched = ipc
            .handle(
                "open_in_mpv",
                json!({
                    "url": video.to_string_lossy(),
                    "title": "Real mpv test",
                    "startTime": 15.0,
                    "media": {"key": "7:8:9", "presence": {"details": "Real"}}
                }),
            )
            .unwrap();
        let session_id = launched["sessionId"].as_str().unwrap().to_owned();

        let mut first = None;
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let state = ipc.handle("get_mpv_state", json!({})).unwrap();
            if state["ready"] == true && state["position"].as_f64().is_some() {
                first = Some(state);
                break;
            }
        }
        let first = first.expect("mpv never became ready");
        eprintln!("first sample: {first}");
        assert_eq!(first["sessionId"], session_id);
        assert_eq!(first["paused"], false);
        assert!(first["duration"].as_f64().unwrap() > 100.0);
        let start = first["position"].as_f64().unwrap();
        assert!(start >= 14.0, "--start was ignored: {start}");

        std::thread::sleep(std::time::Duration::from_secs(3));
        let later = ipc.handle("get_mpv_state", json!({})).unwrap();
        let moved = later["position"].as_f64().unwrap() - start;
        eprintln!("position advanced by {moved:.2}s");
        assert!(moved > 1.5, "playback did not advance: {moved}");

        // Closing mpv from outside must end the session and keep the position.
        if std::env::var_os("ANIVOX_QUIT_VIA_IPC").is_some() {
            // What `q` does in mpv: a clean quit through the JSON IPC socket.
            use std::io::Write;
            let socket = std::fs::read_dir(std::env::temp_dir())
                .unwrap()
                .flatten()
                .map(|entry| entry.path().join("ipc"))
                .find(|path| path.exists())
                .expect("mpv ipc socket");
            let mut stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
            stream
                .write_all(
                    b"{\"command\":[\"quit\"]}
",
                )
                .unwrap();
        } else {
            assert!(std::process::Command::new("pkill")
                .args(["-x", "mpv"])
                .status()
                .unwrap()
                .success());
        }
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if ipc.handle("get_mpv_state", json!({})).unwrap().is_null() {
                break;
            }
        }
        assert!(ipc.handle("get_mpv_state", json!({})).unwrap().is_null());
        let resume = ipc
            .handle("get_playback_resume", json!({"key": "7:8:9"}))
            .unwrap();
        eprintln!("resume point: {resume}");
        assert!(resume["position"].as_f64().unwrap() >= start);
        let quit_cleanly = std::env::var_os("ANIVOX_QUIT_VIA_IPC").is_some();
        while let Ok(event) = rx.try_recv() {
            eprintln!("ui event: {event:?}");
            assert!(!quit_cleanly, "clean quit raised a status: {event:?}");
        }
    }
}
