//! C ABI over `anivox-core` for the WinUI 3 shell (`windows-winui/`).
//!
//! Strings cross the boundary as NUL-terminated UTF-8. Every string returned
//! through an out-pointer is owned by Rust and must be released with
//! [`anivox_free`]. Calls returning `i32` give 0 on success (out = JSON value)
//! and 1 on failure (out = error message). UI events arrive as JSON through the
//! callback passed to [`anivox_create`], possibly on a background thread.

pub mod ipc;
pub mod segments;
pub mod session;
pub mod site_load;

use anivox_core::site_hosts;
use serde_json::Value;
use std::ffi::{c_char, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Arc;

pub type EventCallback = extern "C" fn(event_json: *const c_char, user: *mut c_void);

/// Site scripts in the order the Tauri shell injects them on Windows
/// (`src-tauri/src/lib.rs`), after the IPC shim and the restore guard.
const SITE_SCRIPTS: [&str; 7] = [
    include_str!("../../../src-tauri/scripts/inject-windows.js"),
    include_str!("../../../src-tauri/scripts/mpv-session.js"),
    include_str!("../../../src-tauri/scripts/inject-common.js"),
    include_str!("../../../src-tauri/scripts/player-fullscreen.js"),
    include_str!("../../../src-tauri/scripts/common-styles.js"),
    include_str!("../../../src-tauri/scripts/site-lifecycle.js"),
    include_str!("../../../src-tauri/scripts/segment-monitor.js"),
];
const IPC_SHIM: &str = include_str!("../../scripts/ipc-shim.js");

pub struct Anivox {
    ipc: ipc::Ipc,
}

impl Anivox {
    fn session_path(&self) -> PathBuf {
        self.ipc.host().config_dir().join("session-winui.json")
    }

    fn tray_path(&self) -> PathBuf {
        self.ipc.host().config_dir().join("tray.json")
    }
}

/// Full document-created script for the content webview: every part runs only
/// in the top document of a trusted site origin.
pub fn content_script(startup_url: &str) -> String {
    let origins = site_hosts::js_origins();
    let mut parts = vec![IPC_SHIM.replace("__ANIVOX_ORIGINS__", &origins)];
    if let Some(guard) = url::Url::parse(startup_url)
        .ok()
        .and_then(|url| session::restore_guard_script(&url))
    {
        parts.push(guard);
    }
    for script in SITE_SCRIPTS {
        parts.push(format!(
            "if (window.top === window && {origins}.includes(location.origin)) {{\n{script}\n}}"
        ));
    }
    parts.join("\n;\n")
}

fn read_str<'a>(value: *const c_char) -> Option<&'a str> {
    if value.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(value) }.to_str().ok()
}

fn into_raw(text: String) -> *mut c_char {
    CString::new(text.replace('\0', ""))
        .unwrap_or_default()
        .into_raw()
}

/// Writes the result to `out` and returns the status code.
fn finish(out: *mut *mut c_char, result: Result<Value, String>) -> i32 {
    let (code, text) = match result {
        Ok(value) => (0, value.to_string()),
        Err(err) => (1, err),
    };
    if !out.is_null() {
        unsafe { *out = into_raw(text) };
    }
    code
}

fn guarded(out: *mut *mut c_char, body: impl FnOnce() -> Result<Value, String>) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(body))
        .unwrap_or_else(|_| Err("Внутренняя ошибка ядра Anivox".into()));
    finish(out, result)
}

fn handle<'a>(anivox: *const Anivox) -> Result<&'a Anivox, String> {
    unsafe { anivox.as_ref() }.ok_or_else(|| "Ядро не создано".into())
}

/// Creates the core. `config_dir` holds settings, history and session files
/// (`%APPDATA%\com.yasin.anivox`, shared with the Tauri build); `data_dir` holds
/// MPV logs. Returns null on failure.
#[no_mangle]
pub extern "C" fn anivox_create(
    config_dir: *const c_char,
    data_dir: *const c_char,
    callback: EventCallback,
    user: *mut c_void,
) -> *mut Anivox {
    let created = catch_unwind(|| {
        let config = PathBuf::from(read_str(config_dir)?);
        let data = PathBuf::from(read_str(data_dir)?);
        let user = user as usize;
        let sink: ipc::UiSink = Arc::new(move |event| {
            if let Ok(json) = serde_json::to_string(&event) {
                if let Ok(text) = CString::new(json) {
                    callback(text.as_ptr(), user as *mut c_void);
                }
            }
        });
        let ipc = ipc::Ipc::new(ipc::WinHost::new(sink, config, data)).ok()?;
        Some(Box::into_raw(Box::new(Anivox { ipc })))
    });
    created.ok().flatten().unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn anivox_destroy(anivox: *mut Anivox) {
    if !anivox.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(anivox) })));
    }
}

#[no_mangle]
pub extern "C" fn anivox_free(text: *mut c_char) {
    if !text.is_null() {
        drop(unsafe { CString::from_raw(text) });
    }
}

/// Runs a site command (`set_discord_rpc`, `open_in_mpv`, ...). Blocking: call
/// it off the UI thread. The shell must check the sender origin first.
#[no_mangle]
pub extern "C" fn anivox_invoke(
    anivox: *const Anivox,
    command: *const c_char,
    args_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    guarded(out, || {
        let anivox = handle(anivox)?;
        let command = read_str(command).ok_or("Нет команды")?;
        let args = match read_str(args_json) {
            Some(text) if !text.is_empty() => {
                serde_json::from_str(text).map_err(|err| err.to_string())?
            }
            _ => Value::Object(Default::default()),
        };
        anivox.ipc.handle(command, args)
    })
}

/// The page currently shown in the content webview (null when unknown).
#[no_mangle]
pub extern "C" fn anivox_set_page(anivox: *const Anivox, uri: *const c_char) {
    if let Ok(anivox) = handle(anivox) {
        anivox.ipc.host().set_page(read_str(uri).map(str::to_owned));
    }
}

/// Starts a new load attempt of the site document and returns its generation.
#[no_mangle]
pub extern "C" fn anivox_begin_load(anivox: *const Anivox) -> u64 {
    handle(anivox)
        .ok()
        .and_then(|anivox| {
            let load = anivox.ipc.host().site_load();
            let mut info = load.lock().ok()?;
            Some(info.begin())
        })
        .unwrap_or(0)
}

/// 1 when `url` is an exact https site origin (`anivox.fun`, `anivox.me`).
#[no_mangle]
pub extern "C" fn anivox_is_site_url(url: *const c_char) -> i32 {
    read_str(url)
        .and_then(|url| url::Url::parse(url).ok())
        .is_some_and(|url| site_hosts::is_site_origin(&url)) as i32
}

/// Loads `session-winui.json`; the value carries an extra `startupUrl`.
#[no_mangle]
pub extern "C" fn anivox_session_load(anivox: *const Anivox, out: *mut *mut c_char) -> i32 {
    guarded(out, || {
        let anivox = handle(anivox)?;
        let data = session::load(&anivox.session_path())?;
        let mut value = serde_json::to_value(&data).map_err(|err| err.to_string())?;
        value["startupUrl"] = Value::from(data.startup_url().to_string());
        Ok(value)
    })
}

/// Saves `session-winui.json` atomically; untrusted pages are dropped.
#[no_mangle]
pub extern "C" fn anivox_session_save(
    anivox: *const Anivox,
    session_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    guarded(out, || {
        let anivox = handle(anivox)?;
        let data: session::SessionData =
            serde_json::from_str(read_str(session_json).ok_or("Нет данных сеанса")?)
                .map_err(|err| err.to_string())?;
        session::save_atomic(&anivox.session_path(), &data)?;
        Ok(Value::Null)
    })
}

/// "Close to tray" preference from the shared `tray.json` (default on).
#[no_mangle]
pub extern "C" fn anivox_tray_load(anivox: *const Anivox) -> i32 {
    handle(anivox)
        .ok()
        .map(|anivox| anivox_core::tray_settings::load(&anivox.tray_path()).unwrap_or(true))
        .unwrap_or(true) as i32
}

#[no_mangle]
pub extern "C" fn anivox_tray_save(
    anivox: *const Anivox,
    enabled: i32,
    out: *mut *mut c_char,
) -> i32 {
    guarded(out, || {
        let anivox = handle(anivox)?;
        anivox_core::tray_settings::save(&anivox.tray_path(), enabled != 0)?;
        Ok(Value::Null)
    })
}

/// Document-created script for the content webview (`startup_url` decides
/// whether the restored page starts paused).
#[no_mangle]
pub extern "C" fn anivox_content_script(startup_url: *const c_char, out: *mut *mut c_char) -> i32 {
    guarded(out, || {
        Ok(Value::from(content_script(
            read_str(startup_url).unwrap_or(""),
        )))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_site_script_is_origin_guarded() {
        let script = content_script("https://anivox.fun/anime/1-x");
        assert!(script.starts_with("// Exposes"));
        assert!(script.contains("__anivoxRestoreToken"));
        assert_eq!(
            script.matches("includes(location.origin)) {").count(),
            SITE_SCRIPTS.len()
        );
        assert!(!content_script("https://anivox.fun/").contains("__anivoxRestoreToken"));
    }

    extern "C" fn ignore(_: *const c_char, _: *mut c_void) {}

    #[test]
    fn invoke_round_trip_through_the_c_abi() {
        let dir = tempfile::tempdir().unwrap();
        let config = CString::new(dir.path().join("c").to_string_lossy().as_ref()).unwrap();
        let data = CString::new(dir.path().join("d").to_string_lossy().as_ref()).unwrap();
        let anivox = anivox_create(config.as_ptr(), data.as_ptr(), ignore, std::ptr::null_mut());
        assert!(!anivox.is_null());
        let mut out = std::ptr::null_mut();
        let cmd = CString::new("get_mpv_state").unwrap();
        assert_eq!(
            anivox_invoke(anivox, cmd.as_ptr(), std::ptr::null(), &mut out),
            0
        );
        assert_eq!(unsafe { CStr::from_ptr(out) }.to_str().unwrap(), "null");
        anivox_free(out);
        let bad = CString::new("nope").unwrap();
        assert_eq!(
            anivox_invoke(anivox, bad.as_ptr(), std::ptr::null(), &mut out),
            1
        );
        anivox_free(out);
        assert_eq!(anivox_begin_load(anivox), 1);
        assert_eq!(anivox_tray_load(anivox), 1);
        anivox_destroy(anivox);
    }

    #[test]
    fn site_url_check_is_exact() {
        let ok = CString::new("https://anivox.me/anime/1").unwrap();
        let bad = CString::new("https://anivox.me.evil.test/").unwrap();
        assert_eq!(anivox_is_site_url(ok.as_ptr()), 1);
        assert_eq!(anivox_is_site_url(bad.as_ptr()), 0);
        assert_eq!(anivox_is_site_url(std::ptr::null()), 0);
    }
}
