//! Site command dispatcher behind the WebKit message handler: the shared
//! [`anivox_core::site_ipc`]. Commands are blocking and must be called off the
//! GTK main thread; anything that touches widgets reaches the main thread as a
//! [`UiEvent`] through an async channel.

use anivox_core::site_ipc::EventSink;
pub use anivox_core::site_ipc::{SiteEvent as UiEvent, SiteHost, SiteIpc as Ipc};
use std::path::PathBuf;
use std::sync::Arc;

pub const USER_AGENT: &str = concat!("Anivox/", env!("CARGO_PKG_VERSION"));

pub fn host(
    ui: async_channel::Sender<UiEvent>,
    config_dir: PathBuf,
    data_dir: PathBuf,
) -> SiteHost {
    let sink: EventSink = Arc::new(move |event: UiEvent| {
        let _ = ui.send_blocking(event);
    });
    SiteHost::new(sink, USER_AGENT, config_dir, data_dir)
}

/// Shell-specific checks; the dispatcher's own tests live in `anivox-core`.
#[cfg(all(test, unix))]
mod discord_tests {
    use super::*;
    use anivox_core::mpv_session::MpvHost;
    use serde_json::json;

    fn ipc(dir: &std::path::Path) -> (Ipc, async_channel::Receiver<UiEvent>) {
        let (tx, rx) = async_channel::unbounded();
        (
            Ipc::new(host(tx, dir.join("config"), dir.join("data"))).unwrap(),
            rx,
        )
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
    fn shikimori_activity_is_remembered_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, _rx) = ipc(dir.path());
        std::env::set_var("XDG_RUNTIME_DIR", dir.path());
        // No Discord here: the send fails, but the overlay state is kept.
        let _ = ipc.set_shikimori_activity(Some("Листает Shikimori".into()));
        assert!(ipc.shikimori_activity().is_some());
        // The embedded player keeps its status but does not override Shikimori.
        let args = json!({"presence": {"details": "Аниме", "stateText": "Серия 1"}});
        ipc.handle("set_discord_rpc", args).unwrap();
        assert_eq!(ipc.host().mpv().embedded.lock().unwrap().details, "Аниме");
        let _ = ipc.set_shikimori_activity(None);
        assert!(ipc.shikimori_activity().is_none());
    }
}

#[cfg(all(test, unix))]
mod mpv_flow_tests {
    use super::*;
    use serde_json::{json, Value};
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
        let host = host(tx, config, dir.path().join("data"));
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
        let host = host(tx, dir.path().join("config"), dir.path().join("data"));
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
