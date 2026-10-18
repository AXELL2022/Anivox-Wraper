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
            mpv_path: Some("relative.exe".into())
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
