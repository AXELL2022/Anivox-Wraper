//! Trusted local dropdowns. These are child webviews, never native windows.
use std::sync::Mutex;
use tauri::{Emitter, Manager};

#[derive(Default)]
pub(crate) struct OverlayState {
    pub operation: tokio::sync::Mutex<()>,
    active: Mutex<Option<(String, f64)>>,
}
fn anchor(value: Option<f64>) -> f64 {
    value
        .filter(|n| n.is_finite())
        .unwrap_or(0.98)
        .clamp(0.0, 1.0)
}
pub(crate) fn layout(window: &tauri::Window) -> Result<(), String> {
    let state = window.state::<OverlayState>();
    let active = state.active.lock().map_err(|e| e.to_string())?.clone();
    if let Some((label, anchor)) = active {
        if let Some(view) = window.get_webview(&label) {
            let size = window
                .inner_size()
                .map_err(|e| e.to_string())?
                .to_logical::<f64>(window.scale_factor().map_err(|e| e.to_string())?);
            view.set_bounds(tauri::Rect {
                position: tauri::LogicalPosition::new(0.0, crate::TOOLBAR_HEIGHT).into(),
                size: tauri::LogicalSize::new(
                    size.width,
                    (size.height - crate::TOOLBAR_HEIGHT).max(1.0),
                )
                .into(),
            })
            .map_err(|e| e.to_string())?;
            view.eval(format!("window.__anivoxOverlayAnchor?.({anchor})"))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn report(app: &tauri::AppHandle, label: Option<&str>) {
    #[cfg(target_os = "linux")]
    let target = "content";
    #[cfg(not(target_os = "linux"))]
    let target = "toolbar";
    let _ = app.emit_to(
        target,
        "local-overlay-status",
        serde_json::json!({"label":label}),
    );
}
pub(crate) fn close(app: &tauri::AppHandle) -> Result<(), String> {
    // Keep review/progress attached while the installer owns its operation.
    let update = app.state::<crate::UpdateState>();
    let _guard = update
        .operation
        .try_lock()
        .map_err(|_| "Дождитесь завершения обновления")?;
    for label in ["settings", "update-review"] {
        if let Some(view) = app.get_webview(label) {
            view.close().map_err(|e| e.to_string())?;
        }
    }
    *app.state::<OverlayState>()
        .active
        .lock()
        .map_err(|e| e.to_string())? = None;
    report(app, None);
    if let Some(content) = app.get_webview("content") {
        let _ = content.set_focus();
    }
    Ok(())
}
pub(crate) async fn toggle(
    app: &tauri::AppHandle,
    label: &str,
    page: &str,
    position: Option<f64>,
) -> Result<(), String> {
    let state = app.state::<OverlayState>();
    let _guard = state.operation.lock().await;
    let same = state
        .active
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .is_some_and(|(current, _)| current == label);
    close(app)?;
    if same {
        return Ok(());
    }
    let content = app.get_webview("content").ok_or("Окно сайта недоступно")?;
    let window = content.window();
    let dev = app.config().build.dev_url.clone();
    let view = window
        .add_child(
            tauri::webview::WebviewBuilder::new(label, tauri::WebviewUrl::App(page.into()))
                .transparent(true)
                .background_color(tauri::webview::Color(0, 0, 0, 0))
                .focused(false)
                .additional_browser_args(crate::browser_args_for_platform())
                .on_navigation(move |url| crate::is_local_app_url(url, dev.as_ref())),
            tauri::LogicalPosition::new(0.0, crate::TOOLBAR_HEIGHT),
            tauri::LogicalSize::new(1.0, 1.0),
        )
        .map_err(|e| e.to_string())?;
    *state.active.lock().map_err(|e| e.to_string())? = Some((label.into(), anchor(position)));
    #[cfg(target_os = "linux")]
    {
        if let Err(err) = async {
            crate::attach_linux_shikimori(&content).await?;
            crate::attach_linux_shikimori(&view).await
        }
        .await
        {
            let _ = close(app);
            return Err(err);
        }
    }
    if let Err(err) = layout(&window) {
        let _ = close(app);
        return Err(err);
    }
    report(app, Some(label));
    let _ = view.set_focus();
    Ok(())
}
#[tauri::command]
pub(crate) fn local_overlay_ready(webview: tauri::Webview) -> Result<(), String> {
    if !matches!(webview.label(), "settings" | "update-review") {
        return Err("Недопустимая панель".into());
    }
    crate::ensure_local_window(&webview, webview.label())?;
    layout(&webview.window())
}
#[cfg(test)]
mod tests {
    #[test]
    fn anchors_are_finite_and_bounded() {
        assert_eq!(super::anchor(Some(f64::NAN)), 0.98);
        assert_eq!(super::anchor(Some(-4.0)), 0.0);
        assert_eq!(super::anchor(Some(4.0)), 1.0);
    }
}
