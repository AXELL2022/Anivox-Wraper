//! Window placement and the last Anivox page (`session-winui.json`).
//!
//! Separate from the Tauri `session.json` and GTK `session-gtk.json`. Pages pass
//! `session_page::trusted_page`, so sign-in and token URLs are never stored.

use anivox_core::session_page::{trusted_page, HOME_URL};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use url::Url;

pub const MIN_WIDTH: i32 = 420;
pub const MIN_HEIGHT: i32 = 300;
const MAX_SIDE: i32 = 16384;
const MAX_COORD: i32 = 65536;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StartupPage {
    Home,
    #[default]
    LastPage,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SessionData {
    pub startup_page: StartupPage,
    pub last_page: Option<String>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub maximized: bool,
}

fn valid_size(width: i32, height: i32) -> bool {
    (MIN_WIDTH..=MAX_SIDE).contains(&width) && (MIN_HEIGHT..=MAX_SIDE).contains(&height)
}

impl SessionData {
    pub fn sanitize(&mut self) {
        self.last_page = self
            .last_page
            .as_deref()
            .and_then(trusted_page)
            .map(|url| url.to_string());
        if !matches!((self.width, self.height), (Some(w), Some(h)) if valid_size(w, h)) {
            self.width = None;
            self.height = None;
        }
        if !matches!((self.x, self.y), (Some(x), Some(y)) if x.abs() <= MAX_COORD && y.abs() <= MAX_COORD)
        {
            self.x = None;
            self.y = None;
        }
    }

    pub fn startup_url(&self) -> Url {
        if self.startup_page == StartupPage::LastPage {
            if let Some(url) = self.last_page.as_deref().and_then(trusted_page) {
                return url;
            }
        }
        Url::parse(HOME_URL).expect("constant home URL")
    }
}

/// Missing or corrupt files fall back to defaults; genuine I/O errors propagate.
pub fn load(path: &Path) -> Result<SessionData, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(SessionData::default()),
        Err(err) => return Err(err.to_string()),
    };
    let mut data: SessionData = serde_json::from_slice(&bytes).unwrap_or_default();
    data.sanitize();
    Ok(data)
}

pub fn save_atomic(path: &Path, data: &SessionData) -> Result<(), String> {
    let parent = path.parent().ok_or("Не определён каталог сеанса")?;
    std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let mut sanitized = data.clone();
    sanitized.sanitize();
    let bytes = serde_json::to_vec_pretty(&sanitized).map_err(|err| err.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|err| err.to_string())?;
    file.write_all(&bytes).map_err(|err| err.to_string())?;
    file.as_file().sync_all().map_err(|err| err.to_string())?;
    file.persist(path).map_err(|err| err.error.to_string())?;
    Ok(())
}

/// A restored page is opened for context, not as permission to start playback:
/// `restore-paused.js` blocks media until a trusted click or key press. The
/// token keeps the page unlocked across reloads within the same launch.
pub fn restore_guard_script(startup: &Url) -> Option<String> {
    if startup.as_str() == HOME_URL {
        return None;
    }
    let token = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Some(format!(
        "if ({origins}.includes(location.origin) && sessionStorage.getItem('anivox-restore-unlocked') !== '{token}') {{ window.__anivoxRestoreToken = '{token}'; {script} }}",
        origins = anivox_core::site_hosts::js_origins(),
        script = include_str!("../../../src-tauri/scripts/restore-paused.js"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_pages_and_bad_geometry_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-winui.json");
        let data = SessionData {
            last_page: Some("https://anivox.fun/login".into()),
            x: Some(10),
            y: Some(i32::MAX),
            width: Some(100),
            height: Some(800),
            ..SessionData::default()
        };
        save_atomic(&path, &data).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.last_page, None);
        assert_eq!(
            (loaded.x, loaded.y, loaded.width, loaded.height),
            (None, None, None, None)
        );
        assert_eq!(loaded.startup_url().as_str(), HOME_URL);
    }

    #[test]
    fn last_page_is_restored_with_a_guard() {
        let data = SessionData {
            last_page: Some("https://anivox.fun/anime/42-x?episode=3".into()),
            ..SessionData::default()
        };
        let url = data.startup_url();
        assert_eq!(url.as_str(), "https://anivox.fun/anime/42-x?episode=3");
        assert!(restore_guard_script(&url)
            .unwrap()
            .contains("__anivoxRestoreToken"));
        assert!(restore_guard_script(&Url::parse(HOME_URL).unwrap()).is_none());
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(load(&path).unwrap(), SessionData::default());
    }
}
