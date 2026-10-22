//! Native-owned loading watchdog. A timeout is not proof of a network failure.
#[cfg(test)]
mod tests {
    use super::{is_external_document, LoadInfo};
    #[test]
    fn blank_and_engine_error_documents_cannot_dismiss_loading() {
        for value in [
            "about:blank",
            "about:srcdoc",
            "chrome-error://chromewebdata/",
            "data:text/html,",
            "https://anivox.fun/",
        ] {
            assert!(
                !is_external_document(&tauri::Url::parse(value).unwrap()),
                "{value}"
            );
        }
        assert!(is_external_document(
            &tauri::Url::parse("https://accounts.google.com/").unwrap()
        ));
    }
    #[test]
    fn old_reload_completion_and_watchdog_cannot_finish_current_attempt() {
        let mut state = LoadInfo {
            generation: 2,
            ..LoadInfo::default()
        };
        assert!(!state.finish(1));
        assert!(!state.timeout(1));
        assert_eq!(state.phase, "loading");
        assert!(state.finish(2));
        assert!(!state.timeout(2));
        assert_eq!(state.phase, "ready");
    }
    #[test]
    fn slow_document_can_finish_after_timeout_without_a_reload() {
        let mut state = LoadInfo {
            generation: 3,
            ..LoadInfo::default()
        };
        assert!(state.timeout(3));
        assert_eq!(state.phase, "timeout");
        assert!(state.finish(3));
        assert_eq!(state.phase, "ready");
    }
}
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use tauri::{Emitter, Manager};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoadInfo {
    pub phase: &'static str,
    pub detail: Option<String>,
    pub generation: u64,
}
impl LoadInfo {
    fn finish(&mut self, generation: u64) -> bool {
        if self.generation != generation {
            return false;
        }
        self.phase = "ready";
        self.detail = None;
        true
    }
    fn timeout(&mut self, generation: u64) -> bool {
        if self.generation != generation || self.phase != "loading" {
            return false;
        }
        self.phase = "timeout";
        true
    }
}
impl Default for LoadInfo {
    fn default() -> Self {
        Self {
            phase: "loading",
            detail: None,
            generation: 0,
        }
    }
}
#[derive(Default)]
pub(crate) struct SiteLoadState {
    info: Mutex<LoadInfo>,
    geometry_generation: AtomicU64,
    dismissed_generation: AtomicU64,
}
fn sync_visibility(app: &tauri::AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let state = handle.state::<SiteLoadState>();
        let info = state.info.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(view) = handle.get_webview("site-recovery") {
            if info.phase == "ready"
                || state.dismissed_generation.load(Ordering::Relaxed) == info.generation
            {
                let _ = view.hide();
            } else {
                let _ = view.show();
            }
        }
    });
}
fn emit(app: &tauri::AppHandle, info: &LoadInfo) {
    let _ = app.emit_to("site-recovery", "site-load-state", info);
    #[cfg(target_os = "linux")]
    let target = "content";
    #[cfg(not(target_os = "linux"))]
    let target = "toolbar";
    let _ = app.emit_to(target, "site-load-state", info);
}
pub(crate) fn snapshot(app: &tauri::AppHandle) -> LoadInfo {
    app.state::<SiteLoadState>()
        .info
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}
pub(crate) fn begin(app: &tauri::AppHandle) {
    let info = {
        let state = app.state::<SiteLoadState>();
        let mut info = state.info.lock().unwrap_or_else(|e| e.into_inner());
        info.generation += 1;
        info.phase = "loading";
        info.detail = None;
        info.clone()
    };
    emit(app, &info);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Navigation callbacks may run before the local child exists at startup.
        sync_visibility(&app);
        tokio::time::sleep(std::time::Duration::from_secs(25)).await;
        let timed_out = {
            let state = app.state::<SiteLoadState>();
            let mut current = state.info.lock().unwrap_or_else(|e| e.into_inner());
            if current.timeout(info.generation) {
                Some(current.clone())
            } else {
                None
            }
        };
        if let Some(current) = timed_out {
            emit(&app, &current);
            sync_visibility(&app);
        }
    });
}
pub(crate) fn is_external_document(url: &tauri::Url) -> bool {
    matches!(url.scheme(), "https" | "http")
        && url.host_str().is_some()
        && url.host_str() != Some("anivox.fun")
}
pub(crate) fn ready(app: &tauri::AppHandle) {
    finish(app, snapshot(app).generation);
}
fn finish(app: &tauri::AppHandle, generation: u64) {
    let info = {
        let state = app.state::<SiteLoadState>();
        let mut info = state.info.lock().unwrap_or_else(|e| e.into_inner());
        if !info.finish(generation) {
            return;
        }
        info.clone()
    };
    emit(app, &info);
    sync_visibility(app);
}
pub(crate) fn error(app: &tauri::AppHandle, detail: String) {
    let info = {
        let state = app.state::<SiteLoadState>();
        let mut info = state.info.lock().unwrap_or_else(|e| e.into_inner());
        info.phase = "error";
        info.detail = Some(detail);
        info.clone()
    };
    emit(app, &info);
    sync_visibility(app);
}
pub(crate) fn layout(window: &tauri::Window) -> tauri::Result<()> {
    if let Some(view) = window.get_webview("site-recovery") {
        let size = window
            .inner_size()?
            .to_logical::<f64>(window.scale_factor()?);
        let top = if cfg!(target_os = "linux") {
            0.0
        } else {
            crate::TOOLBAR_HEIGHT
        };
        view.set_bounds(tauri::Rect {
            position: tauri::LogicalPosition::new(0.0, top).into(),
            size: tauri::LogicalSize::new(size.width, (size.height - top).max(1.0)).into(),
        })?;
    }
    Ok(())
}
pub(crate) async fn create(app: &tauri::AppHandle) -> Result<(), String> {
    let content = app.get_webview("content").ok_or("Окно сайта недоступно")?;
    let window = content.window();
    let view = window
        .add_child(
            tauri::webview::WebviewBuilder::new(
                "site-recovery",
                tauri::WebviewUrl::App("site-recovery.html".into()),
            )
            .additional_browser_args(crate::browser_args_for_platform())
            .on_navigation({
                let dev = app.config().build.dev_url.clone();
                move |url| crate::is_local_app_url(url, dev.as_ref())
            }),
            tauri::LogicalPosition::new(0.0, crate::TOOLBAR_HEIGHT),
            tauri::LogicalSize::new(1280.0, 680.0),
        )
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    {
        crate::attach_linux_shikimori(&content).await?;
        crate::attach_linux_shikimori(&view).await?;
        view.with_webview(|platform| {
            use gtk::prelude::*;
            platform.inner().set_margin_top(0);
        })
        .map_err(|e| e.to_string())?;
    }
    layout(&window).map_err(|e| e.to_string())?;
    sync_visibility(app);
    let _ = view;
    Ok(())
}
#[tauri::command]
pub(crate) fn get_site_load_state(
    app: tauri::AppHandle,
    webview: tauri::Webview,
) -> Result<LoadInfo, String> {
    crate::ensure_local_window(&webview, "site-recovery")?;
    Ok(snapshot(&app))
}
#[tauri::command]
pub(crate) async fn site_recovery_action(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    action: String,
) -> Result<(), String> {
    crate::ensure_local_window(&webview, "site-recovery")?;
    if action == "wait" {
        let state = app.state::<SiteLoadState>();
        let generation = state
            .info
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .generation;
        state
            .dismissed_generation
            .store(generation, Ordering::Relaxed);
        sync_visibility(&app);
        return Ok(());
    }
    let content = app.get_webview("content").ok_or("Окно сайта недоступно")?;
    let url = match action.as_str() {
        "home" => "https://anivox.fun/".parse().unwrap(),
        "retry" => app
            .state::<crate::BrowserState>()
            .url
            .lock()
            .map_err(|e| e.to_string())?
            .clone(),
        _ => return Err("Неизвестное действие восстановления".into()),
    };
    begin(&app);
    if let Err(err) = content.navigate(url) {
        error(&app, err.to_string());
        return Err(err.to_string());
    }
    Ok(())
}
#[tauri::command]
pub(crate) fn get_site_document_generation(
    app: tauri::AppHandle,
    webview: tauri::Webview,
) -> Result<u64, String> {
    let native = webview.url().map_err(|e| e.to_string())?;
    if webview.label() != "content"
        || native.origin() != tauri::Url::parse("https://anivox.fun/").unwrap().origin()
    {
        return Err("Недопустимый источник состояния сайта".into());
    }
    Ok(snapshot(&app).generation)
}

#[tauri::command]
pub(crate) fn site_document_state(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    href: String,
    loaded: bool,
    generation: u64,
) -> Result<(), String> {
    let native = webview.url().map_err(|e| e.to_string())?;
    if webview.label() != "content"
        || native.origin() != tauri::Url::parse("https://anivox.fun/").unwrap().origin()
    {
        return Err("Недопустимый источник состояния сайта".into());
    }
    let url = tauri::Url::parse(&href).map_err(|e| e.to_string())?;
    if url.origin() != native.origin() {
        return Err("Недопустимый адрес сайта".into());
    }
    // A late document-ready from a previous native navigation must not hide recovery.
    if url.path() != native.path() || url.query() != native.query() {
        return Ok(());
    }
    if snapshot(&app).generation != generation {
        return Ok(());
    }
    if loaded {
        finish(&app, generation);
    }
    crate::session::persist_last_page(&app, &url).map(|_| ())
}
pub(crate) fn schedule_geometry_save(window: &tauri::Window) {
    if let Err(err) = crate::session::capture_geometry(window) {
        eprintln!("Не удалось запомнить окно: {err}");
    }
    let app = window.app_handle().clone();
    let generation = app
        .state::<SiteLoadState>()
        .geometry_generation
        .fetch_add(1, Ordering::Relaxed)
        + 1;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        if app
            .state::<SiteLoadState>()
            .geometry_generation
            .load(Ordering::Relaxed)
            == generation
        {
            if let Err(err) = crate::session::flush(&app) {
                eprintln!("Не удалось сохранить состояние окна: {err}");
            }
        }
    });
}
