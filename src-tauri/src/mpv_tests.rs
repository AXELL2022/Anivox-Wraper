use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "anivox-settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn missing_settings_use_path_and_corrupt_settings_are_preserved() {
    let dir = TempDir::new();
    let path = dir.0.join("settings.json");
    assert!(load_player_settings(&path).unwrap().mpv_path.is_none());
    std::fs::write(&path, b"{broken").unwrap();
    assert!(load_player_settings(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
}

#[test]
fn settings_validate_and_roundtrip_unicode_paths() {
    let dir = TempDir::new();
    let exe = dir.0.join("Плеер с пробелом.exe");
    std::fs::write(&exe, b"test").unwrap();
    let settings = PlayerSettings {
        mpv_path: validate_mpv_path(Some(exe.to_string_lossy().into_owned())).unwrap(),
        ..Default::default()
    };
    let path = dir.0.join("nested/settings.json");
    save_player_settings(&path, &settings).unwrap();
    assert_eq!(
        load_player_settings(&path).unwrap().mpv_path,
        settings.mpv_path
    );
    save_player_settings(&path, &PlayerSettings::default()).unwrap();
    assert!(load_player_settings(&path).unwrap().mpv_path.is_none());
    assert_eq!(validate_mpv_path(Some("  ".into())).unwrap(), None);
    assert!(validate_mpv_path(Some("relative.exe".into())).is_err());
    assert!(validate_mpv_path(Some(dir.0.to_string_lossy().into_owned())).is_err());
    assert!(validate_mpv_path(Some(
        dir.0.join("missing.exe").to_string_lossy().into_owned()
    ))
    .is_err());
}

#[test]
fn failed_settings_write_preserves_previous_content() {
    let dir = TempDir::new();
    let path = dir.0.join("settings.json");
    std::fs::write(&path, b"previous").unwrap();
    assert!(save_player_settings(
        &path,
        &PlayerSettings {
            mpv_path: Some("relative.exe".into()),
            ..Default::default()
        }
    )
    .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"previous");
    let blocked = dir.0.join("blocked.json");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(blocked.join("sentinel"), b"keep").unwrap();
    assert!(save_player_settings(&blocked, &PlayerSettings::default()).is_err());
    assert_eq!(std::fs::read(blocked.join("sentinel")).unwrap(), b"keep");
}

#[test]
fn mpv_command_preserves_metadata_and_uses_app_log_path() {
    let log = PathBuf::from("журналы с пробелом/mpv.log");
    let custom = PlayerSettings {
        mpv_path: Some("custom mpv.exe".into()),
        ..Default::default()
    };
    let command = build_mpv_command(
        &custom,
        &log,
        "https://cdn.example/stream.m3u8".into(),
        Some("Серия 1".into()),
        Some(42.5),
        Some("https://cdn.example/sub.vtt".into()),
        Some("test-token".into()),
    );
    assert_eq!(command.get_program(), "custom mpv.exe");
    assert!(command.get_current_dir().is_none());
    let args: Vec<_> = command
        .get_args()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    for expected in [
        "--force-media-title=Серия 1",
        "--start=42.50",
        "--sub-file=https://cdn.example/sub.vtt",
        "--http-header-fields-append=Authorization: Bearer test-token",
        "--referrer=https://anivox.fun/",
        "--log-file=журналы с пробелом/mpv.log",
    ] {
        assert!(args.contains(&expected.to_string()), "missing {expected}");
    }
    assert!(!args.iter().any(|s| s.contains("F:\\Mpv")));
    let default = build_mpv_command(
        &PlayerSettings::default(),
        &log,
        "https://example.org/stream".into(),
        None,
        None,
        None,
        None,
    );
    assert_eq!(default.get_program(), "mpv");
}

#[test]
fn auxiliary_windows_do_not_have_primary_lifecycle_or_remote_access() {
    assert!(is_primary_window("main"));
    assert!(is_primary_window("content"));
    for label in ["settings", "update-review", "shikimori"] {
        assert!(!is_primary_window(label));
    }
    let local = tauri::Url::parse("http://tauri.localhost/settings.html").unwrap();
    assert!(local_window_access("settings", &local, "settings", None).is_ok());
    for label in ["content", "toolbar", "shikimori", "update-review"] {
        assert!(local_window_access(label, &local, "settings", None).is_err());
    }
    let remote = tauri::Url::parse("https://anivox.fun/").unwrap();
    assert!(local_window_access("settings", &remote, "settings", None).is_err());
}

#[cfg(debug_assertions)]
#[test]
fn local_windows_accept_only_the_configured_loopback_dev_origin() {
    let dev = tauri::Url::parse("http://127.0.0.1:1430/").unwrap();
    for (label, page) in [
        ("settings", "settings.html"),
        ("update-review", "update-review.html"),
    ] {
        let url = dev.join(page).unwrap();
        assert!(
            is_local_app_url(&url, Some(&dev)),
            "configured dev page must load"
        );
        assert!(local_window_access(label, &url, label, Some(&dev)).is_ok());
        assert!(!is_local_app_url(&url, None));
        assert!(local_window_access("content", &url, label, Some(&dev)).is_err());
    }
    for denied in [
        "http://127.0.0.1:1431/settings.html",
        "https://127.0.0.1:1430/settings.html",
        "http://localhost:1430/settings.html",
        "http://127.0.0.1.evil.example:1430/settings.html",
        "http://user:password@127.0.0.1:1430/settings.html",
        "https://anivox.fun/settings.html",
    ] {
        assert!(
            !is_local_app_url(&tauri::Url::parse(denied).unwrap(), Some(&dev)),
            "must deny {denied}"
        );
    }
    let external_dev = tauri::Url::parse("https://anivox.fun/").unwrap();
    assert!(!is_local_app_url(
        &external_dev.join("settings.html").unwrap(),
        Some(&external_dev)
    ));
}

#[cfg(not(debug_assertions))]
#[test]
fn release_local_windows_do_not_trust_dev_origins() {
    let dev = tauri::Url::parse("http://127.0.0.1:1430/").unwrap();
    assert!(!is_local_app_url(
        &dev.join("settings.html").unwrap(),
        Some(&dev)
    ));
}

#[test]
fn newer_update_requires_review_and_preserves_its_notes() {
    assert!(changed_update_review("0.2.5", "0.2.5", None).is_none());
    let outcome = changed_update_review("0.2.5", "0.2.6", Some("New notes".into())).unwrap();
    assert_eq!(
        serde_json::to_value(outcome).unwrap(),
        serde_json::json!({
            "update_changed": { "version": "0.2.6", "body": "New notes" }
        })
    );
}

#[test]
fn mpv_snapshot_handles_nulls_pause_and_speed_without_inventing_position() {
    let mut sample = MpvPlayback::default();
    sample.apply(&serde_json::json!({"event":"property-change","name":"time-pos","data":42.5}));
    sample.apply(&serde_json::json!({"event":"property-change","name":"duration","data":1200}));
    sample.apply(&serde_json::json!({"event":"property-change","name":"pause","data":false}));
    sample.apply(&serde_json::json!({"event":"property-change","name":"speed","data":1.5}));
    sample.apply(&serde_json::json!({"event":"file-loaded"}));
    assert!(sample.ready);
    assert_eq!(sample.position, Some(42.5));
    assert!(!sample.paused);
    sample.apply(&serde_json::json!({"event":"property-change","name":"time-pos","data":null}));
    assert_eq!(sample.position, Some(42.5));
    sample.apply(&serde_json::json!({"event":"end-file","reason":"eof"}));
    assert!(sample.ended);
    assert!(sample.paused);
}

#[test]
fn resume_history_uses_episode_identity_and_rejects_near_end_or_invalid_values() {
    let mut history = PlaybackHistory::default();
    history.record("1:2:3", 42.5, Some(1200.0));
    assert_eq!(history.resume("1:2:3").unwrap().position, 42.5);
    assert!(history.resume("1:2:4").is_none());
    history.record("1:2:3", 1190.0, Some(1200.0));
    assert!(history.resume("1:2:3").is_none());
    history.record("1:3:3", f64::NAN, Some(1200.0));
    assert!(history.resume("1:3:3").is_none());
    assert!(!valid_media_key("../../settings.json"));
    assert!(valid_media_key("123:4:5"));
}

#[test]
fn mpv_seek_back_from_eof_can_resume() {
    let mut sample = MpvPlayback::default();
    sample.apply(&serde_json::json!({"event":"property-change","name":"eof-reached","data":true}));
    assert!(sample.ended);
    sample.apply(&serde_json::json!({"event":"property-change","name":"eof-reached","data":false}));
    assert!(!sample.ended);
}

#[test]
fn superseded_session_cannot_overwrite_same_episode_history() {
    let state = MpvState::default();
    let sample = |id: &str| MpvSession {
        session_id: id.into(),
        media_key: "1:2:3".into(),
        mirror_embedded: false,
        playback: MpvPlayback::default(),
        media: None,
    };
    state
        .progress_owners
        .lock()
        .unwrap()
        .insert("1:2:3".into(), "new".into());
    assert!(!state.owns_progress(&sample("old")));
    assert!(state.owns_progress(&sample("new")));
}

#[tokio::test]
#[ignore = "requires ANIVOX_TEST_MPV pointing to a real MPV executable"]
async fn real_mpv_ipc_reports_pause_seek_duration_and_speed() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let executable = std::env::var_os("ANIVOX_TEST_MPV").expect("Set ANIVOX_TEST_MPV");
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("fixture.wav");
    let length = 120u32 * 8000 * 2;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + length).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(8000u32.to_le_bytes());
    bytes.extend(16000u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(length.to_le_bytes());
    bytes.resize(bytes.len() + length as usize, 0);
    std::fs::write(&wav, bytes).unwrap();
    #[cfg(target_os = "windows")]
    let endpoint = format!(r"\\.\pipe\anivox-integration-{}", std::process::id());
    #[cfg(not(target_os = "windows"))]
    let endpoint = dir.path().join("ipc").to_string_lossy().into_owned();
    let mut command = Command::new(executable);
    command.args([
        "--no-config",
        "--no-terminal",
        "--ao=null",
        "--vo=null",
        "--pause=yes",
        "--keep-open=yes",
    ]);
    command
        .arg(format!("--input-ipc-server={endpoint}"))
        .arg(&wav);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = tokio::process::Command::from(command)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stream = None;
    for _ in 0..60 {
        match connect_mpv_ipc(&endpoint).await {
            Ok(value) => {
                stream = Some(value);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
        }
    }
    let mut stream = stream.expect("MPV IPC connection");
    observe_mpv(&mut stream).await.unwrap();
    stream.write_all(b"{\"command\":[\"set_property\",\"speed\",1.5]}\n{\"command\":[\"seek\",20,\"absolute\"]}\n{\"command\":[\"set_property\",\"pause\",false]}\n").await.unwrap();
    let mut reader = tokio::io::BufReader::new(stream);
    let mut sample = MpvPlayback::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        let mut line = String::new();
        if let Ok(Ok(n)) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            reader.read_line(&mut line),
        )
        .await
        {
            if n == 0 {
                break;
            }
            if let Ok(event) = serde_json::from_str(&line) {
                sample.apply(&event);
            }
            if sample.duration.is_some_and(|n| (n - 120.0).abs() < 1.0)
                && sample.position.is_some_and(|n| n >= 20.0)
                && !sample.paused
                && sample.speed == 1.5
            {
                break;
            }
        }
    }
    assert!(sample.duration.is_some_and(|n| (n - 120.0).abs() < 1.0));
    assert!(sample.position.is_some_and(|n| n >= 20.0));
    assert!(!sample.paused);
    assert_eq!(sample.speed, 1.5);
    let path = dir.path().join("history.json");
    let mut history = PlaybackHistory::default();
    history.record("1:2:3", sample.position.unwrap(), sample.duration);
    write_history(&path, &history).unwrap();
    assert!(
        read_history(&path)
            .unwrap()
            .resume("1:2:3")
            .unwrap()
            .position
            >= 20.0
    );
    reader
        .get_mut()
        .write_all(b"{\"command\":[\"quit\"]}\n")
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[test]
fn mpv_discord_timer_follows_speed_and_disappears_on_pause_or_buffering() {
    let mut sample = MpvSession {
        session_id: "test".into(),
        media_key: "1:2:3".into(),
        mirror_embedded: true,
        media: Some(PlaybackMedia {
            key: "1:2:3".into(),
            presence: DiscordPresence {
                details: "Frozen anime".into(),
                state_text: "Серия 2".into(),
                ..Default::default()
            },
        }),
        playback: MpvPlayback {
            ready: true,
            paused: false,
            position: Some(100.0),
            duration: Some(200.0),
            speed: 2.0,
            ..Default::default()
        },
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let presence = mpv_presence(&sample);
    assert_eq!(presence.details, "Frozen anime");
    assert!(presence
        .end_timestamp
        .is_some_and(|end| (now + 49..=now + 51).contains(&end)));
    assert!(presence.state_text.contains("MPV"));
    sample.playback.paused = true;
    assert!(mpv_presence(&sample).end_timestamp.is_none());
    sample.playback.paused = false;
    sample.playback.buffering = true;
    assert!(mpv_presence(&sample).end_timestamp.is_none());
    let exposed = serde_json::to_value(&sample).unwrap();
    assert!(exposed.get("media").is_none());
}

#[test]
fn old_settings_default_mirror_off_and_new_setting_is_persisted() {
    let old: PlayerSettings = serde_json::from_str(r#"{"mpvPath":null}"#).unwrap();
    assert!(!old.mirror_embedded);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    save_player_settings(
        &path,
        &PlayerSettings {
            mirror_embedded: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(load_player_settings(&path).unwrap().mirror_embedded);
}
