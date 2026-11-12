//! Window size, maximized state and the last Anivox page (`session-gtk.json`).
//!
//! The file is separate from the Tauri `session.json`: Wayland does not let
//! clients place windows, so only the size is stored and the formats differ.
//! [`Session`] lives on the GTK main thread. Writes are atomic; size changes
//! are buffered in memory and flushed after a short idle period, before hiding
//! to the tray and on exit.

use anivox_core::session_page::{trusted_page, HOME_URL};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::path::{Path, PathBuf};
use url::Url;

pub const MIN_WIDTH: i32 = 420;
pub const MIN_HEIGHT: i32 = 300;
const MAX_SIDE: i32 = 16384;

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
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub maximized: bool,
}

fn valid_size(width: i32, height: i32) -> bool {
    (MIN_WIDTH..=MAX_SIDE).contains(&width) && (MIN_HEIGHT..=MAX_SIDE).contains(&height)
}

impl SessionData {
    fn sanitize(&mut self) {
        self.last_page = self
            .last_page
            .as_deref()
            .and_then(trusted_page)
            .map(|url| url.to_string());
        if !matches!((self.width, self.height), (Some(w), Some(h)) if valid_size(w, h)) {
            self.width = None;
            self.height = None;
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

    pub fn size(&self) -> Option<(i32, i32)> {
        Some((self.width?, self.height?)).filter(|(w, h)| valid_size(*w, *h))
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
    if let Ok(directory) = std::fs::File::open(parent) {
        let _ = directory.sync_all();
    }
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
        script = include_str!("../../src-tauri/scripts/restore-paused.js"),
    ))
}

pub struct Session {
    path: PathBuf,
    data: RefCell<SessionData>,
    dirty: Cell<bool>,
    /// Bumped on every buffered change; a delayed flush runs only for the latest.
    generation: Cell<u64>,
}

impl Session {
    pub fn open(path: PathBuf) -> Self {
        let data = load(&path).unwrap_or_else(|err| {
            eprintln!("Не удалось прочитать сеанс: {err}");
            SessionData::default()
        });
        Self {
            path,
            data: RefCell::new(data),
            dirty: Cell::new(false),
            generation: Cell::new(0),
        }
    }

    pub fn data(&self) -> SessionData {
        self.data.borrow().clone()
    }

    /// Records the window state in memory. Fullscreen and minimized states never
    /// replace the normal size; a maximized window keeps the previous normal size.
    pub fn observe_window(&self, width: i32, height: i32, maximized: bool, fullscreen: bool) {
        if fullscreen {
            return;
        }
        let mut data = self.data.borrow_mut();
        let before = data.clone();
        data.maximized = maximized;
        if !maximized && valid_size(width, height) {
            data.width = Some(width);
            data.height = Some(height);
        }
        if *data != before {
            self.dirty.set(true);
            self.generation.set(self.generation.get() + 1);
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation.get()
    }

    /// Remembers a trusted page and writes it at once. Returns whether it was accepted.
    pub fn set_last_page(&self, uri: &str) -> bool {
        let Some(url) = trusted_page(uri) else {
            return false;
        };
        let page = url.to_string();
        if self.data.borrow().last_page.as_deref() != Some(page.as_str()) {
            self.data.borrow_mut().last_page = Some(page);
            self.dirty.set(true);
        }
        if let Err(err) = self.flush() {
            eprintln!("Не удалось сохранить сеанс: {err}");
        }
        true
    }

    pub fn set_startup_page(&self, page: StartupPage) -> Result<(), String> {
        if self.data.borrow().startup_page != page {
            self.data.borrow_mut().startup_page = page;
            self.dirty.set(true);
        }
        self.flush()
    }

    pub fn flush(&self) -> Result<(), String> {
        if self.dirty.get() {
            save_atomic(&self.path, &self.data.borrow())?;
            self.dirty.set(false);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_corrupt_files_default_to_home() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-gtk.json");
        assert_eq!(load(&path).unwrap(), SessionData::default());
        std::fs::write(&path, "{broken").unwrap();
        let data = load(&path).unwrap();
        assert_eq!(data.startup_url().as_str(), HOME_URL);
    }

    #[test]
    fn last_page_and_size_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/session-gtk.json");
        let session = Session::open(path.clone());
        assert!(session.set_last_page("https://anivox.fun/anime/42-x?episode=2"));
        assert!(!session.set_last_page("https://anivox.fun/login"));
        assert!(!session.set_last_page("https://anivox.me/anime/1"));
        session.observe_window(1400, 900, false, false);
        session.observe_window(3000, 2000, true, false);
        session.observe_window(100, 100, false, true);
        session.flush().unwrap();

        let reopened = Session::open(path);
        let data = reopened.data();
        assert_eq!(
            data.startup_url().as_str(),
            "https://anivox.fun/anime/42-x?episode=2"
        );
        assert_eq!(data.size(), Some((1400, 900)));
        assert!(data.maximized);
    }

    #[test]
    fn home_startup_ignores_the_last_page_and_needs_no_guard() {
        let dir = tempfile::tempdir().unwrap();
        let session = Session::open(dir.path().join("s.json"));
        session.set_last_page("https://anivox.fun/anime/7");
        session.set_startup_page(StartupPage::Home).unwrap();
        let url = session.data().startup_url();
        assert_eq!(url.as_str(), HOME_URL);
        assert!(restore_guard_script(&url).is_none());
        let restored = Url::parse("https://anivox.fun/anime/7").unwrap();
        let guard = restore_guard_script(&restored).unwrap();
        assert!(guard.contains("__anivoxRestoreToken"));
        assert!(guard.contains("https://anivox.me"));
    }

    #[test]
    fn invalid_sizes_are_dropped() {
        let data: SessionData =
            serde_json::from_str(r#"{"width": 10, "height": 99999, "lastPage": "javascript:x"}"#)
                .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        save_atomic(&path, &data).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.size(), None);
        assert_eq!(loaded.last_page, None);
    }
}
