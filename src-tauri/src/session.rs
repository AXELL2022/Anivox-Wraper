//! Native session persistence. Integrate from setup/events; no remote-page IPC is required.
//!
//! `initialize` runs before creating the primary window. `restore_geometry` runs after
//! native creation, before installing event listeners/adding child webviews. On primary
//! move/resize events call `capture_geometry` (memory only). Call `flush` before tray
//! hiding and application exit; an optional parent-owned idle timer can flush after
//! a drag. Navigation calls `persist_last_page` only for the `content` webview.
//! All writes are serialized and atomic; there is deliberately no detached debounce
//! worker that could outlive shutdown or overwrite newer settings.

use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::Manager;

pub const HOME_URL: &str = "https://anivox.fun/";
const MIN_WIDTH: f64 = 420.0;
const MIN_HEIGHT: f64 = 300.0;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StartupPage {
    Home,
    #[default]
    LastPage,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SessionSettings {
    pub startup_page: StartupPage,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
pub struct LogicalSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PhysicalPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub name: Option<String>,
    /// Physical desktop work area, not the full display (excludes taskbars).
    pub position: PhysicalPosition,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NormalGeometry {
    pub inner_size: LogicalSize,
    pub outer_position: PhysicalPosition,
    pub scale_factor: f64,
    pub monitor: Option<MonitorInfo>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SessionData {
    pub settings: SessionSettings,
    pub last_page: Option<String>,
    pub normal: Option<NormalGeometry>,
    pub maximized: bool,
}

impl SessionData {
    pub fn startup_url(&self) -> tauri::Url {
        if self.settings.startup_page == StartupPage::LastPage {
            if let Some(url) = self.last_page.as_deref().and_then(trusted_page) {
                return url;
            }
        }
        tauri::Url::parse(HOME_URL).expect("constant home URL")
    }

    /// Fullscreen/minimized observations never replace the last normal bounds or
    /// maximized preference. In particular fullscreen does not become maximized.
    pub fn observe(&mut self, sample: GeometrySample) {
        if sample.fullscreen || sample.minimized {
            return;
        }
        self.maximized = sample.maximized;
        if !sample.maximized {
            if let Some(normal) = sample.normal.filter(valid_geometry) {
                self.normal = Some(normal);
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct GeometrySample {
    pub normal: Option<NormalGeometry>,
    pub maximized: bool,
    pub fullscreen: bool,
    pub minimized: bool,
}

/// Strict origin and conservative page allowlist. Never persist credentials,
/// fragments (OAuth implicit responses), authentication paths or unknown query
/// parameters. Rejected navigation leaves the previous trusted page untouched.
pub fn trusted_page(value: &str) -> Option<tauri::Url> {
    if value.len() > 8192 || value.chars().any(char::is_control) {
        return None;
    }
    let url = tauri::Url::parse(value).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("anivox.fun")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    // Also reject empty userinfo, which URL parsing can otherwise normalize away.
    if value
        .split("://")
        .nth(1)?
        .split(['/', '?', '#'])
        .next()?
        .contains('@')
    {
        return None;
    }
    let path = decode_ascii_path(url.path())?;
    if path
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| {
            matches!(
                part,
                "auth"
                    | "oauth"
                    | "oauth2"
                    | "authorize"
                    | "authorization"
                    | "callback"
                    | "signin"
                    | "signout"
                    | "login"
                    | "logout"
                    | "token"
                    | "tokens"
                    | "password"
                    | "reset"
                    | "verify"
                    | "session"
                    | "sso"
            )
        })
    {
        return None;
    }
    if url.query_pairs().any(|(key, _)| {
        !matches!(
            key.as_ref(),
            "page"
                | "search"
                | "q"
                | "sort"
                | "order"
                | "genre"
                | "year"
                | "season"
                | "status"
                | "type"
                | "episode"
                | "translation"
        )
    }) {
        return None;
    }
    Some(url)
}

fn decode_ascii_path(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            let byte = u8::from_str_radix(hex, 16).ok()?;
            // Reject double encoding rather than trying to interpret server rules.
            if byte == b'%' {
                return None;
            }
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(String::from_utf8(out).ok()?.to_lowercase())
}

fn valid_scale(scale: f64) -> bool {
    scale.is_finite() && (0.25..=8.0).contains(&scale)
}
fn valid_geometry(value: &NormalGeometry) -> bool {
    valid_scale(value.scale_factor)
        && value.inner_size.width.is_finite()
        && value.inner_size.height.is_finite()
        && value.inner_size.width > 0.0
        && value.inner_size.height > 0.0
}
fn valid_monitor(value: &&MonitorInfo) -> bool {
    value.width > 0 && value.height > 0 && valid_scale(value.scale_factor)
}

#[derive(Clone, Debug, PartialEq)]
pub struct RestoreGeometry {
    pub inner_size: LogicalSize,
    pub outer_position: PhysicalPosition,
}

/// Keep logical client size across DPI changes. Preserve physical coordinates only
/// on the same monitor/work area/scale; otherwise center on the selected monitor.
/// No monitors => no placement changes (let the OS choose a safe initial window).
/// `primary` indexes `monitors`; missing/stale primary falls back to the first.
pub fn plan_restore(
    normal: &NormalGeometry,
    monitors: &[MonitorInfo],
    primary: Option<usize>,
) -> Option<RestoreGeometry> {
    if !valid_geometry(normal) {
        return None;
    }
    let fallback = primary
        .and_then(|i| monitors.get(i))
        .filter(|m| valid_monitor(m))
        .or_else(|| monitors.iter().find(valid_monitor))?;
    let matching = normal.monitor.as_ref().and_then(|old| {
        monitors.iter().find(|m| {
            valid_monitor(m)
                && ((old.name.is_some() && old.name == m.name)
                    || (old.position == m.position
                        && old.width == m.width
                        && old.height == m.height))
        })
    });
    let monitor = matching.unwrap_or(fallback);
    let scale = monitor.scale_factor;
    // Conservative decoration reserve (physical pixels scaled from logical DIPs).
    // Position uses outer bounds while Tauri set_size uses inner bounds.
    let available_w = (monitor.width as f64 / scale - 32.0).max(1.0);
    let available_h = (monitor.height as f64 / scale - 64.0).max(1.0);
    let width = normal
        .inner_size
        .width
        .clamp(MIN_WIDTH.min(available_w), available_w);
    let height = normal
        .inner_size
        .height
        .clamp(MIN_HEIGHT.min(available_h), available_h);
    let outer_w = ((width + 32.0) * scale).min(monitor.width as f64);
    let outer_h = ((height + 64.0) * scale).min(monitor.height as f64);
    let left = monitor.position.x as f64;
    let top = monitor.position.y as f64;
    let right = left + monitor.width as f64 - outer_w;
    let bottom = top + monitor.height as f64 - outer_h;
    let unchanged = matching.is_some()
        && normal.monitor.as_ref().is_some_and(|old| {
            old.position == monitor.position
                && old.width == monitor.width
                && old.height == monitor.height
                && (normal.scale_factor - scale).abs() < 0.01
        });
    let (x, y) = if unchanged {
        (
            normal.outer_position.x as f64,
            normal.outer_position.y as f64,
        )
    } else {
        ((left + right) / 2.0, (top + bottom) / 2.0)
    };
    Some(RestoreGeometry {
        inner_size: LogicalSize { width, height },
        outer_position: PhysicalPosition {
            x: x.clamp(left, right).round() as i32,
            y: y.clamp(top, bottom).round() as i32,
        },
    })
}

/// Missing/corrupt/unsupported JSON defaults safely; genuine I/O errors propagate.
pub fn load(path: &Path) -> Result<SessionData, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(SessionData::default()),
        Err(err) => return Err(err.to_string()),
    };
    let mut data: SessionData = serde_json::from_slice(&bytes).unwrap_or_default();
    data.last_page = data
        .last_page
        .as_deref()
        .and_then(trusted_page)
        .map(|url| url.to_string());
    data.normal = data.normal.filter(valid_geometry);
    Ok(data)
}

pub fn save_atomic(path: &Path, data: &SessionData) -> Result<(), String> {
    let parent = path.parent().ok_or("Session path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let mut sanitized = data.clone();
    sanitized.last_page = sanitized
        .last_page
        .as_deref()
        .and_then(trusted_page)
        .map(|url| url.to_string());
    sanitized.normal = sanitized.normal.filter(valid_geometry);
    let bytes = serde_json::to_vec_pretty(&sanitized).map_err(|err| err.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|err| err.to_string())?;
    file.write_all(&bytes).map_err(|err| err.to_string())?;
    file.as_file().sync_all().map_err(|err| err.to_string())?;
    file.persist(path).map_err(|err| err.error.to_string())?;
    // Best-effort directory sync makes rename durable on Unix. Windows does not
    // permit opening directories this way; tempfile uses atomic replacement there.
    #[cfg(unix)]
    if let Ok(directory) = std::fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

struct StoredSession {
    data: SessionData,
    dirty: bool,
}
pub struct SessionState {
    path: PathBuf,
    inner: Mutex<StoredSession>,
}

/// Manage state once in setup; corrupt files default, unreadable files fail setup.
pub fn initialize(app: &tauri::AppHandle) -> Result<tauri::Url, String> {
    let path = app
        .path()
        .app_config_dir()
        .map_err(|err| err.to_string())?
        .join("session.json");
    let data = load(&path)?;
    let url = data.startup_url();
    if !app.manage(SessionState {
        path,
        inner: Mutex::new(StoredSession { data, dirty: false }),
    }) {
        return Err("Session state was already initialized".into());
    }
    Ok(url)
}

pub fn flush(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app
        .try_state::<SessionState>()
        .ok_or("Session state not initialized")?;
    let mut stored = state.inner.lock().map_err(|err| err.to_string())?;
    if stored.dirty {
        save_atomic(&state.path, &stored.data)?;
        stored.dirty = false;
    }
    Ok(())
}

pub fn persist_last_page(app: &tauri::AppHandle, url: &tauri::Url) -> Result<bool, String> {
    let Some(url) = trusted_page(url.as_str()) else {
        return Ok(false);
    };
    let state = app
        .try_state::<SessionState>()
        .ok_or("Session state not initialized")?;
    let mut stored = state.inner.lock().map_err(|err| err.to_string())?;
    let page = url.to_string();
    if stored.data.last_page.as_ref() != Some(&page) {
        stored.data.last_page = Some(page);
        stored.dirty = true;
    }
    if stored.dirty {
        save_atomic(&state.path, &stored.data)?;
        stored.dirty = false;
    }
    Ok(true)
}

fn monitor_info(monitor: &tauri::Monitor) -> MonitorInfo {
    let area = monitor.work_area();
    MonitorInfo {
        name: monitor.name().cloned(),
        position: PhysicalPosition {
            x: area.position.x,
            y: area.position.y,
        },
        width: area.size.width,
        height: area.size.height,
        scale_factor: monitor.scale_factor(),
    }
}

/// Call only for main/content native window, never settings/update-review windows.
pub fn restore_geometry(window: &tauri::Window) -> Result<(), String> {
    let state = window
        .app_handle()
        .try_state::<SessionState>()
        .ok_or("Session state not initialized")?;
    let data = state
        .inner
        .lock()
        .map_err(|err| err.to_string())?
        .data
        .clone();
    if let Some(normal) = &data.normal {
        let monitors: Vec<_> = window
            .available_monitors()
            .map_err(|err| err.to_string())?
            .iter()
            .map(monitor_info)
            .collect();
        let primary = window
            .primary_monitor()
            .map_err(|err| err.to_string())?
            .as_ref()
            .map(monitor_info);
        let primary = primary
            .as_ref()
            .and_then(|p| monitors.iter().position(|m| m == p));
        if let Some(plan) = plan_restore(normal, &monitors, primary) {
            window
                .set_size(tauri::LogicalSize::new(
                    plan.inner_size.width,
                    plan.inner_size.height,
                ))
                .map_err(|err| err.to_string())?;
            window
                .set_position(tauri::PhysicalPosition::new(
                    plan.outer_position.x,
                    plan.outer_position.y,
                ))
                .map_err(|err| err.to_string())?;
        }
    }
    if data.maximized {
        window.maximize().map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// No disk I/O. Uses live native state, not Resized payloads that can describe
/// fullscreen/minimized transitions. Query failures do not destroy saved bounds.
pub fn capture_geometry(window: &tauri::Window) -> Result<(), String> {
    let fullscreen = window.is_fullscreen().map_err(|err| err.to_string())?;
    let minimized = window.is_minimized().map_err(|err| err.to_string())?;
    if fullscreen
        || minimized
        || window
            .state::<crate::BrowserState>()
            .fullscreen
            .load(std::sync::atomic::Ordering::Relaxed)
    {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        if window
            .gtk_window()
            .map_err(|err| err.to_string())?
            .window()
            .is_some_and(|native| native.state().contains(gtk::gdk::WindowState::FULLSCREEN))
        {
            return Ok(());
        }
    }
    let maximized = window.is_maximized().map_err(|err| err.to_string())?;
    let normal = if maximized {
        None
    } else {
        let scale = window.scale_factor().map_err(|err| err.to_string())?;
        let size = window
            .inner_size()
            .map_err(|err| err.to_string())?
            .to_logical::<f64>(scale);
        let pos = window.outer_position().map_err(|err| err.to_string())?;
        Some(NormalGeometry {
            inner_size: LogicalSize {
                width: size.width,
                height: size.height,
            },
            outer_position: PhysicalPosition { x: pos.x, y: pos.y },
            scale_factor: scale,
            monitor: window
                .current_monitor()
                .map_err(|err| err.to_string())?
                .as_ref()
                .map(monitor_info),
        })
    };
    // Avoid recording a transition that happened during native queries.
    if window.is_fullscreen().map_err(|err| err.to_string())?
        || window.is_minimized().map_err(|err| err.to_string())?
        || window.is_maximized().map_err(|err| err.to_string())? != maximized
    {
        return Ok(());
    }
    let state = window
        .app_handle()
        .try_state::<SessionState>()
        .ok_or("Session state not initialized")?;
    let mut stored = state.inner.lock().map_err(|err| err.to_string())?;
    let before = stored.data.clone();
    stored.data.observe(GeometrySample {
        normal,
        maximized,
        fullscreen,
        minimized,
    });
    stored.dirty |= before != stored.data;
    Ok(())
}

fn ensure_settings_access(webview: &tauri::Webview) -> Result<(), String> {
    // Reuse the parent's existing runtime origin/label guard, not capabilities
    // alone. This denies remote content even if permissions are misconfigured.
    crate::ensure_local_window(webview, "settings")
}

#[tauri::command]
pub fn get_session_settings(webview: tauri::Webview) -> Result<SessionSettings, String> {
    ensure_settings_access(&webview)?;
    let state = webview
        .app_handle()
        .try_state::<SessionState>()
        .ok_or("Session state not initialized")?;
    let stored = state.inner.lock().map_err(|err| err.to_string())?;
    Ok(stored.data.settings.clone())
}

#[tauri::command]
pub async fn save_session_settings(
    webview: tauri::Webview,
    startup_page: StartupPage,
) -> Result<SessionSettings, String> {
    ensure_settings_access(&webview)?;
    let app = webview.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app
            .try_state::<SessionState>()
            .ok_or("Session state not initialized")?;
        let mut stored = state.inner.lock().map_err(|err| err.to_string())?;
        let mut next = stored.data.clone();
        next.settings.startup_page = startup_page;
        save_atomic(&state.path, &next)?;
        stored.data = next;
        stored.dirty = false;
        Ok(stored.data.settings.clone())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn monitor(x: i32, scale: f64) -> MonitorInfo {
        MonitorInfo {
            name: Some(format!("display-{x}")),
            position: PhysicalPosition { x, y: 0 },
            width: 1920,
            height: 1080,
            scale_factor: scale,
        }
    }
    fn normal(m: &MonitorInfo) -> NormalGeometry {
        NormalGeometry {
            inner_size: LogicalSize {
                width: 1280.0,
                height: 720.0,
            },
            outer_position: PhysicalPosition {
                x: m.position.x + 20,
                y: 20,
            },
            scale_factor: m.scale_factor,
            monitor: Some(m.clone()),
        }
    }
    #[test]
    fn startup_default_and_home() {
        let mut data = SessionData::default();
        assert_eq!(data.settings.startup_page, StartupPage::LastPage);
        assert_eq!(data.startup_url().as_str(), HOME_URL);
        data.last_page = Some("https://anivox.fun/anime/42".into());
        assert_eq!(data.startup_url().path(), "/anime/42");
        data.settings.startup_page = StartupPage::Home;
        assert_eq!(data.startup_url().as_str(), HOME_URL);
    }
    #[test]
    fn rejects_untrusted_and_auth_urls() {
        for value in [
            "invalid",
            "http://anivox.fun/",
            "https://evil.test/",
            "https://anivox.fun.evil.test/",
            "https://sub.anivox.fun/",
            "https://anivox.fun:444/",
            "https://user@anivox.fun/",
            "https://:secret@anivox.fun/",
            "https://@anivox.fun/",
            "https://anivox.fun/oauth/callback",
            "https://anivox.fun/%6cogin",
            "https://anivox.fun/%256cogin",
            "https://anivox.fun/?access_token=secret",
            "https://anivox.fun/?code=secret",
            "https://anivox.fun/#access_token=secret",
            "https://anivox.fun/token/secret",
            "https://anivox.fun/?redirect_uri=x",
        ] {
            assert!(trusted_page(value).is_none(), "{value}");
        }
        for value in [
            HOME_URL,
            "https://anivox.fun/anime/42",
            "https://anivox.fun/catalog?page=2&sort=year",
        ] {
            assert!(trusted_page(value).is_some(), "{value}");
        }
    }
    #[test]
    fn missing_monitor_and_dpi_changes_are_safe() {
        let old = monitor(-1920, 1.0);
        let saved = normal(&old);
        assert!(plan_restore(&saved, &[], None).is_none());
        let current = monitor(0, 1.5);
        let plan = plan_restore(&saved, &[current], Some(99)).unwrap();
        assert!(plan.outer_position.x >= 0 && plan.outer_position.y >= 0);
        assert!(plan.inner_size.width * 1.5 <= 1920.0);
        assert!(plan.inner_size.height * 1.5 <= 1080.0);
    }
    #[test]
    fn negative_coordinates_are_valid_and_offscreen_is_clamped() {
        let m = monitor(-1920, 1.0);
        let mut saved = normal(&m);
        let plan = plan_restore(&saved, &[m.clone()], Some(0)).unwrap();
        assert_eq!(plan.outer_position.x, -1900);
        saved.outer_position = PhysicalPosition {
            x: i32::MIN,
            y: i32::MAX,
        };
        let plan = plan_restore(&saved, &[m], None).unwrap();
        assert_eq!(plan.outer_position.x, -1920);
        assert!(plan.outer_position.y < 1080);
    }
    #[test]
    fn huge_tiny_and_invalid_sizes() {
        let m = monitor(0, 1.0);
        let mut saved = normal(&m);
        saved.inner_size = LogicalSize {
            width: 1e30,
            height: 1e30,
        };
        let plan = plan_restore(&saved, &[m.clone()], None).unwrap();
        assert_eq!(
            plan.inner_size,
            LogicalSize {
                width: 1888.0,
                height: 1016.0
            }
        );
        saved.inner_size = LogicalSize {
            width: 1.0,
            height: 2.0,
        };
        assert_eq!(
            plan_restore(&saved, &[m.clone()], None).unwrap().inner_size,
            LogicalSize {
                width: 420.0,
                height: 300.0
            }
        );
        saved.inner_size.width = f64::NAN;
        assert!(plan_restore(&saved, &[m.clone()], None).is_none());
        saved = normal(&m);
        saved.scale_factor = 0.0;
        assert!(plan_restore(&saved, &[m], None).is_none());
    }
    #[test]
    fn tiny_monitor_wins_over_minimum_size() {
        let mut m = monitor(0, 2.0);
        m.width = 300;
        m.height = 200;
        let plan = plan_restore(&normal(&m), &[m], None).unwrap();
        assert!(plan.inner_size.width <= 150.0 && plan.inner_size.height <= 100.0);
    }
    #[test]
    fn special_window_states_preserve_normal_bounds() {
        let saved = normal(&monitor(0, 1.0));
        let mut data = SessionData {
            normal: Some(saved.clone()),
            ..Default::default()
        };
        data.observe(GeometrySample {
            maximized: true,
            ..Default::default()
        });
        assert!(data.maximized);
        for sample in [
            GeometrySample {
                fullscreen: true,
                ..Default::default()
            },
            GeometrySample {
                minimized: true,
                ..Default::default()
            },
        ] {
            data.observe(sample);
            assert!(data.maximized);
            assert_eq!(data.normal, Some(saved.clone()));
        }
        data.observe(GeometrySample {
            normal: Some(saved.clone()),
            ..Default::default()
        });
        assert!(!data.maximized);
        assert_eq!(data.normal, Some(saved));
    }
    #[test]
    fn atomic_replace_corrupt_and_missing_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/session.json");
        assert_eq!(load(&path).unwrap(), SessionData::default());
        let mut data = SessionData {
            last_page: Some("https://anivox.fun/anime/42".into()),
            normal: Some(normal(&monitor(-1920, 1.5))),
            maximized: true,
            ..Default::default()
        };
        save_atomic(&path, &data).unwrap();
        assert_eq!(load(&path).unwrap(), data);
        data.settings.startup_page = StartupPage::Home;
        save_atomic(&path, &data).unwrap();
        assert_eq!(load(&path).unwrap(), data);
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        std::fs::write(&path, b"{partial").unwrap();
        assert_eq!(load(&path).unwrap(), SessionData::default());
        std::fs::write(
            &path,
            br#"{"lastPage":"https://evil.test/","maximized":true}"#,
        )
        .unwrap();
        let loaded = load(&path).unwrap();
        assert!(loaded.last_page.is_none());
        assert!(loaded.maximized);
    }
    #[test]
    fn failed_atomic_save_does_not_replace_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        std::fs::create_dir(&path).unwrap();
        assert!(save_atomic(&path, &SessionData::default()).is_err());
        assert!(path.is_dir());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
