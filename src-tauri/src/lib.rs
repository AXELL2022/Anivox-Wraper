// The macOS binary has a separate minimal entry point (macos.rs).
#![cfg(not(target_os = "macos"))]

use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

mod discord;
mod discord_presence;
mod local_overlay;
mod mpv;
mod player_settings;
mod session;
mod site_hosts;
mod site_recovery;
mod tray_settings;
mod updater;
use discord::{update_discord_presence, DiscordPresence, DiscordState, ShikimoriInfoState};
use std::sync::atomic::Ordering;
use tray_settings::TrayState;

const TOOLBAR_HEIGHT: f64 = 40.0;
const SHIKIMORI_MARGIN: f64 = 16.0;
const SHIKIMORI_HEADER: f64 = 40.0;

#[cfg(test)]
mod discord_tests;
#[cfg(test)]
mod mpv_tests;
#[cfg(test)]
mod shikimori_tests;

#[derive(Default)]
struct ShikimoriState {
    visible: std::sync::atomic::AtomicBool,
    operation: tokio::sync::Mutex<()>,
}

#[cfg(target_os = "windows")]
const BROWSER_ARGS: &str = "--use-angle=d3d11 --enable-features=NvidiaVpSuperResolution,NvidiaVpTrueHDR,DirectCompositionVideoOverlays,DirectCompositionLetterboxVideoOptimization,DirectCompositionScalableVideoProcessing,DirectCompositionUseNV12DecodeSwapChain,UseD3D11VideoProcessor,D3D11VideoDecoder,HardwareAcceleratedVideoDecode,AllowDcompOverlaysInBackbuffer --disable-features=msEdgeVideoSuperResolution,msWebOOUI,msPdfOOUI,msSmartScreenProtection --enable-nv12-dxgi-video --ignore-gpu-blocklist --enable-gpu-rasterization --force-high-performance-gpu";

fn browser_args_for_platform() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        BROWSER_ARGS
    }
    #[cfg(not(target_os = "windows"))]
    {
        ""
    }
}

#[cfg(target_os = "windows")]
fn set_windows_taskbar_icon(window: &tauri::Window) -> Result<(), Box<dyn std::error::Error>> {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageW, ICON_BIG, ICON_SMALL, WM_GETICON, WM_SETICON,
    };

    let hwnd = window.hwnd()?;
    // Tauri/Tao assigns the app icon to ICON_SMALL only. Windows can use a
    // different shell fallback for the taskbar unless ICON_BIG is explicit.
    let icon = unsafe { SendMessageW(hwnd, WM_GETICON, Some(WPARAM(ICON_SMALL as usize)), None) };
    if icon.0 == 0 {
        return Err(std::io::Error::other("Application window icon is missing").into());
    }
    // Borrow the same HICON: Tao owns it for the window's lifetime. Do not
    // destroy it here or the title-bar and taskbar icons would become invalid.
    unsafe {
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(LPARAM(icon.0)),
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn add_linux_toolbar_script(script: &str) -> String {
    let markup = serde_json::to_string(include_str!("../../ui/toolbar-fragment.html"))
        .expect("toolbar HTML must serialize");
    let styles = serde_json::to_string(include_str!("../../ui/toolbar-overlay.css"))
        .expect("toolbar CSS must serialize");
    let toolbar_script = include_str!("../../ui/toolbar.js");

    let mut result = String::with_capacity(
        script.len() + markup.len() + styles.len() + toolbar_script.len() + 900,
    );
    result.push_str(script);
    result.push_str(
        r#"
        ;(() => {
            const installToolbar = () => {
                if (document.getElementById('anivox-toolbar-host')) return;
                const spacer = document.createElement('div');
                spacer.id = 'anivox-toolbar-spacer';
                spacer.style.height = '40px';
                spacer.setAttribute('aria-hidden', 'true');
                document.documentElement.insertBefore(spacer, document.body);
                const host = document.createElement('div');
                host.id = 'anivox-toolbar-host';
                const root = host.attachShadow({ mode: 'open' });
                const style = document.createElement('style');
                style.textContent = "#,
    );
    result.push_str(&styles);
    result.push_str(
        r#";
                root.appendChild(style);
                root.innerHTML += "#,
    );
    result.push_str(&markup);
    result.push_str(
        r#";
                document.documentElement.appendChild(host);
                "#,
    );
    result.push_str(toolbar_script);
    result.push_str(
        r#"
            };
            if (document.readyState === 'loading') {
                document.addEventListener('DOMContentLoaded', installToolbar, { once: true });
            } else {
                installToolbar();
            }
        })();
        "#,
    );
    result
}

struct BrowserState {
    url: Mutex<tauri::Url>,
    fullscreen: std::sync::atomic::AtomicBool,
}

#[cfg(target_os = "linux")]
fn layout_webviews(window: &tauri::Window) -> tauri::Result<()> {
    // The site fills the window; only the native local recovery child needs bounds.
    site_recovery::layout(window)?;
    local_overlay::layout(window).map_err(|e| tauri::Error::Anyhow(std::io::Error::other(e).into()))
}

#[cfg(not(target_os = "linux"))]
fn layout_webviews(window: &tauri::Window) -> tauri::Result<()> {
    let size = window
        .inner_size()?
        .to_logical::<f64>(window.scale_factor()?);
    let fullscreen = cfg!(target_os = "windows")
        && window
            .state::<BrowserState>()
            .fullscreen
            .load(std::sync::atomic::Ordering::Relaxed);
    let top = if fullscreen { 0.0 } else { TOOLBAR_HEIGHT };
    if let Some(toolbar) = window.get_webview("toolbar") {
        toolbar.set_bounds(tauri::Rect {
            position: tauri::LogicalPosition::new(0.0, 0.0).into(),
            size: tauri::LogicalSize::new(size.width, TOOLBAR_HEIGHT).into(),
        })?;
        if fullscreen {
            toolbar.hide()?;
        } else {
            toolbar.show()?;
        }
    }
    if let Some(content) = window.get_webview("content") {
        content.set_bounds(tauri::Rect {
            position: tauri::LogicalPosition::new(0.0, top).into(),
            size: tauri::LogicalSize::new(size.width, (size.height - top).max(1.0)).into(),
        })?;
    }
    site_recovery::layout(window)?;
    layout_shikimori(window, top)?;
    local_overlay::layout(window)
        .map_err(|e| tauri::Error::Anyhow(std::io::Error::other(e).into()))?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn layout_shikimori(window: &tauri::Window, top: f64) -> tauri::Result<()> {
    let size = window
        .inner_size()?
        .to_logical::<f64>(window.scale_factor()?);
    if let Some(overlay) = window.get_webview("shikimori-overlay") {
        overlay.set_bounds(tauri::Rect {
            position: tauri::LogicalPosition::new(0.0, top).into(),
            size: tauri::LogicalSize::new(size.width, (size.height - top).max(1.0)).into(),
        })?;
    }
    if let Some(site) = window.get_webview("shikimori") {
        site.set_bounds(tauri::Rect {
            position: tauri::LogicalPosition::new(
                SHIKIMORI_MARGIN,
                top + SHIKIMORI_MARGIN + SHIKIMORI_HEADER,
            )
            .into(),
            size: tauri::LogicalSize::new(
                (size.width - SHIKIMORI_MARGIN * 2.0).max(1.0),
                (size.height - top - SHIKIMORI_MARGIN * 2.0 - SHIKIMORI_HEADER).max(1.0),
            )
            .into(),
        })?;
    }
    Ok(())
}

// Wry packs Linux webviews into a GtkBox. Reparent them into a GtkOverlay
// so the original WebKit view stays alive, visible and at its original size.
#[cfg(target_os = "linux")]
async fn attach_linux_shikimori(webview: &tauri::Webview) -> Result<(), String> {
    let window = webview.window();
    let label = webview.label().to_string();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    webview
        .with_webview(move |platform| {
            use gtk::prelude::*;
            let result = (|| -> Result<(), String> {
                let vbox = window.default_vbox().map_err(|err| err.to_string())?;
                let root = vbox
                    .children()
                    .into_iter()
                    .find(|child| child.widget_name() == "anivox-shikimori-root")
                    .and_then(|child| child.downcast::<gtk::Overlay>().ok());
                let widget = platform.inner();
                let root = if let Some(root) = root {
                    root
                } else {
                    if label != "content" {
                        return Err("Контейнер Shikimori не создан".into());
                    }
                    let root = gtk::Overlay::new();
                    root.set_widget_name("anivox-shikimori-root");
                    vbox.remove(&widget);
                    root.add(&widget);
                    vbox.pack_start(&root, true, true, 0);
                    widget.show();
                    root.show();
                    root
                };
                if label == "content"
                    || widget
                        .parent()
                        .is_some_and(|parent| parent.widget_name() == "anivox-shikimori-root")
                {
                    return Ok(());
                }
                vbox.remove(&widget);
                widget.set_halign(gtk::Align::Fill);
                widget.set_valign(gtk::Align::Fill);
                // Clear the size request set when this view was created in the GtkBox.
                widget.set_size_request(-1, -1);
                if label == "shikimori" {
                    widget.set_margin_start(SHIKIMORI_MARGIN as i32);
                    widget.set_margin_end(SHIKIMORI_MARGIN as i32);
                    widget.set_margin_top(
                        (TOOLBAR_HEIGHT + SHIKIMORI_MARGIN + SHIKIMORI_HEADER) as i32,
                    );
                    widget.set_margin_bottom(SHIKIMORI_MARGIN as i32);
                } else {
                    widget.set_margin_top(TOOLBAR_HEIGHT as i32);
                }
                root.add_overlay(&widget);
                Ok(())
            })();
            let _ = sender.send(result);
        })
        .map_err(|err| err.to_string())?;
    receiver.await.map_err(|err| err.to_string())?
}

fn report_shikimori(app: &tauri::AppHandle, visible: bool) {
    #[cfg(target_os = "linux")]
    let target = "content";
    #[cfg(not(target_os = "linux"))]
    let target = "toolbar";
    let _ = app.emit_to(
        target,
        "shikimori-status",
        serde_json::json!({ "visible": visible }),
    );
}

fn unload_shikimori(app: &tauri::AppHandle) -> Result<bool, String> {
    for label in ["shikimori", "shikimori-overlay"] {
        if let Some(view) = app.get_webview(label) {
            view.close().map_err(|err| err.to_string())?;
        }
    }
    app.state::<ShikimoriState>()
        .visible
        .store(false, std::sync::atomic::Ordering::Relaxed);
    report_shikimori(app, false);
    if let Some(content) = app.get_webview("content") {
        let _ = content.set_focus();
    }
    Ok(false)
}

fn shikimori_navigation_allowed(url: &tauri::Url) -> bool {
    // Social sign-in redirects away from Shikimori and back. IPC permissions
    // remain origin-scoped in capabilities/shikimori.json.
    url.scheme() == "https"
}

fn shikimori_new_window_response(
    url: &tauri::Url,
) -> tauri::webview::NewWindowResponse<tauri::Wry> {
    if shikimori_navigation_allowed(url) {
        // Let the engine create the popup so cookies and window.opener are
        // preserved for OAuth; opening it in the system browser would not.
        tauri::webview::NewWindowResponse::Allow
    } else {
        tauri::webview::NewWindowResponse::Deny
    }
}

async fn show_shikimori(app: &tauri::AppHandle) -> Result<bool, String> {
    let content = app.get_webview("content").ok_or("Окно сайта недоступно")?;
    let window = content.window();
    let size = window
        .inner_size()
        .map_err(|err| err.to_string())?
        .to_logical::<f64>(window.scale_factor().map_err(|err| err.to_string())?);
    if app.get_webview("shikimori-overlay").is_none() {
        let overlay = window
            .add_child(
                tauri::webview::WebviewBuilder::new(
                    "shikimori-overlay",
                    tauri::WebviewUrl::App("shikimori.html".into()),
                )
                .transparent(true)
                .additional_browser_args(browser_args_for_platform()),
                tauri::LogicalPosition::new(0.0, TOOLBAR_HEIGHT),
                tauri::LogicalSize::new(size.width, (size.height - TOOLBAR_HEIGHT).max(1.0)),
            )
            .map_err(|err| err.to_string())?;
        overlay.hide().map_err(|err| err.to_string())?;
    }
    if app.get_webview("shikimori").is_none() {
        let site = window
            .add_child(
                tauri::webview::WebviewBuilder::new(
                    "shikimori",
                    tauri::WebviewUrl::External(shikimori_overlay_url(
                        &content.url().map_err(|err| err.to_string())?,
                    )),
                )
                .initialization_script(include_str!("../scripts/shikimori.js"))
                .additional_browser_args(browser_args_for_platform())
                .on_navigation(shikimori_navigation_allowed)
                .on_new_window(|url, _features| shikimori_new_window_response(&url)),
                tauri::LogicalPosition::new(
                    SHIKIMORI_MARGIN,
                    TOOLBAR_HEIGHT + SHIKIMORI_MARGIN + SHIKIMORI_HEADER,
                ),
                tauri::LogicalSize::new(
                    (size.width - SHIKIMORI_MARGIN * 2.0).max(1.0),
                    (size.height - TOOLBAR_HEIGHT - SHIKIMORI_MARGIN * 2.0 - SHIKIMORI_HEADER)
                        .max(1.0),
                ),
            )
            .map_err(|err| err.to_string())?;
        site.hide().map_err(|err| err.to_string())?;
    }
    let overlay = app
        .get_webview("shikimori-overlay")
        .ok_or("Панель Shikimori недоступна")?;
    let site = app.get_webview("shikimori").ok_or("Shikimori недоступен")?;
    #[cfg(target_os = "linux")]
    {
        attach_linux_shikimori(&content).await?;
        attach_linux_shikimori(&overlay).await?;
        attach_linux_shikimori(&site).await?;
    }
    layout_webviews(&window).map_err(|err| err.to_string())?;
    overlay.show().map_err(|err| err.to_string())?;
    site.show().map_err(|err| err.to_string())?;
    app.state::<ShikimoriState>()
        .visible
        .store(true, std::sync::atomic::Ordering::Relaxed);
    report_shikimori(app, true);
    let _ = site.set_focus();
    Ok(true)
}

#[tauri::command]
async fn shikimori_action(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    action: String,
) -> Result<bool, String> {
    if webview.label() != "shikimori-overlay" {
        ensure_toolbar_access(&webview)?;
    }
    let state = app.state::<ShikimoriState>();
    let _operation = state.operation.lock().await;
    let visible = state.visible.load(std::sync::atomic::Ordering::Relaxed);
    match action.as_str() {
        "status" => Ok(visible),
        "close" => unload_shikimori(&app),
        "toggle" if visible => unload_shikimori(&app),
        "toggle" => match show_shikimori(&app).await {
            Ok(visible) => Ok(visible),
            Err(err) => {
                let _ = unload_shikimori(&app);
                Err(err)
            }
        },
        "back" | "forward" | "reload" => {
            let site = app
                .get_webview("shikimori")
                .ok_or("Shikimori ещё не открыт")?;
            let script = match action.as_str() {
                "back" => "window.history.back()",
                "forward" => "window.history.forward()",
                _ => "window.location.reload()",
            };
            site.eval(script).map_err(|err| err.to_string())?;
            Ok(visible)
        }
        _ => Err("Неизвестное действие Shikimori".into()),
    }
}

#[tauri::command]
async fn close_shikimori(app: tauri::AppHandle, webview: tauri::Webview) -> Result<bool, String> {
    if webview.label() != "shikimori" {
        return Err("Эта команда доступна только Shikimori".into());
    }
    let state = app.state::<ShikimoriState>();
    let _operation = state.operation.lock().await;
    unload_shikimori(&app)
}

#[tauri::command]
async fn content_fullscreen(window: tauri::Window, fullscreen: bool) -> Result<(), String> {
    window
        .state::<BrowserState>()
        .fullscreen
        .store(fullscreen, std::sync::atomic::Ordering::Relaxed);
    layout_webviews(&window).map_err(|err| err.to_string())
}

#[tauri::command]
fn report_player_status(
    app: tauri::AppHandle,
    text: String,
    color: String,
    reset_after_ms: u64,
    detail: Option<String>,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let target = "content";
    #[cfg(not(target_os = "linux"))]
    let target = "toolbar";
    app.emit_to(
        target,
        "player-status",
        serde_json::json!({
            "text": text, "color": color, "resetAfterMs": reset_after_ms,
            "detail": detail
        }),
    )
    .map_err(|err| err.to_string())
}

// Keep the WebviewWindow adapter compiled on every host so Windows checks also
// catch signature regressions in the Linux builder callback.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn handle_content_window_load(
    window: tauri::WebviewWindow,
    payload: tauri::webview::PageLoadPayload<'_>,
) {
    let webview: &tauri::Webview = window.as_ref();
    handle_content_load(webview.clone(), payload);
}

fn handle_content_load(webview: tauri::Webview, payload: tauri::webview::PageLoadPayload<'_>) {
    if matches!(payload.event(), tauri::webview::PageLoadEvent::Started) {
        tauri::async_runtime::spawn(async move {
            let _ = content_fullscreen(webview.window(), false).await;
        });
    } else if site_recovery::is_external_document(payload.url()) {
        // Only a real external HTTP(S) document (e.g. login) may complete here.
        // WebView2 also finishes about:blank/error documents during startup/failure;
        // those must never dismiss recovery or cancel its watchdog.
        site_recovery::ready(webview.app_handle());
    }
}

#[tauri::command]
async fn browser_action(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    action: String,
) -> Result<(), String> {
    ensure_toolbar_access(&webview)?;
    let content = app.get_webview("content").ok_or("Окно сайта недоступно")?;
    let result = match action.as_str() {
        "back" => content.eval("window.history.back()"),
        "forward" => content.eval("window.history.forward()"),
        "reload" => {
            site_recovery::begin(&app);
            let last_url = app
                .state::<BrowserState>()
                .url
                .lock()
                .map_err(|err| err.to_string())?
                .clone();
            let url = content
                .url()
                .ok()
                .filter(|url| matches!(url.scheme(), "https" | "http"))
                .unwrap_or(last_url);
            content.navigate(url)
        }
        "mpv" => content.eval("window.__anivoxLaunchMpv?.()"),
        _ => return Err("Неизвестное действие панели".into()),
    };
    result.map_err(|err| err.to_string())
}

#[tauri::command]
fn get_app_version(app: tauri::AppHandle) -> String {
    format!("v{}", app.package_info().version)
}

fn ensure_toolbar_access(webview: &tauri::Webview) -> Result<(), String> {
    let expected_label = if cfg!(target_os = "linux") {
        "content"
    } else {
        "toolbar"
    };
    if webview.label() == expected_label {
        Ok(())
    } else {
        Err("Эта команда доступна только с панели приложения".into())
    }
}

fn shikimori_overlay_url(content: &tauri::Url) -> tauri::Url {
    discord::site_title_id(content)
        .and_then(|id| tauri::Url::parse(&format!("https://shikimori.io/animes/{id}")).ok())
        .unwrap_or_else(|| tauri::Url::parse("https://shikimori.io/").expect("static URL"))
}

fn is_primary_window(label: &str) -> bool {
    matches!(label, "main" | "content")
}

fn is_local_app_url(url: &tauri::Url, dev_url: Option<&tauri::Url>) -> bool {
    if matches!(
        (url.scheme(), url.host_str()),
        ("tauri", Some("localhost")) | ("http" | "https", Some("tauri.localhost"))
    ) {
        return true;
    }
    // `tauri dev` serves ui/ on a configured loopback origin. It is local
    // application content too, but arbitrary localhost ports must stay denied.
    if !cfg!(debug_assertions) || !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    dev_url.is_some_and(|dev| {
        let loopback = dev.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        loopback && matches!(dev.scheme(), "http" | "https") && url.origin() == dev.origin()
    })
}

fn local_window_access(
    label: &str,
    url: &tauri::Url,
    expected: &str,
    dev_url: Option<&tauri::Url>,
) -> Result<(), String> {
    if label == expected && is_local_app_url(url, dev_url) {
        Ok(())
    } else {
        Err("Эта команда доступна только локальному окну приложения".into())
    }
}

fn ensure_local_window(webview: &tauri::Webview, expected: &str) -> Result<(), String> {
    local_window_access(
        webview.label(),
        &webview.url().map_err(|err| err.to_string())?,
        expected,
        webview.app_handle().config().build.dev_url.as_ref(),
    )
}

#[tauri::command]
async fn open_settings(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    anchor: Option<f64>,
) -> Result<(), String> {
    ensure_toolbar_access(&webview)?;
    local_overlay::toggle(&app, "settings", "settings.html", anchor).await
}

#[tauri::command]
async fn open_connection(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    anchor: Option<f64>,
) -> Result<(), String> {
    ensure_toolbar_access(&webview)?;
    local_overlay::toggle(&app, "connection", "connection.html", anchor).await
}

#[derive(Default)]
struct SegmentStatsState(Mutex<Option<SegmentStats>>);

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SegmentStats {
    host: String,
    active: bool,
    inflight: u32,
    stalled_ms: u64,
    has_speed: bool,
    mbps: f64,
    slow: u32,
    failed: u32,
    samples: u32,
    max_ms: u64,
    updated_at: f64,
}

/// Окно сайта присылает сводку скорости загрузки видео-сегментов (.ts и т.п.).
#[tauri::command]
fn report_segment_stats(
    webview: tauri::Webview,
    state: tauri::State<SegmentStatsState>,
    mut stats: SegmentStats,
) -> Result<(), String> {
    if webview.label() != "content" {
        return Err("Эта команда доступна только окну сайта".into());
    }
    stats.host = stats.host.chars().take(120).collect();
    for value in [&mut stats.mbps] {
        if !value.is_finite() || *value < 0.0 {
            *value = 0.0;
        }
    }
    *state.0.lock().map_err(|e| e.to_string())? = Some(stats);
    Ok(())
}

#[tauri::command]
fn get_segment_stats(
    webview: tauri::Webview,
    state: tauri::State<SegmentStatsState>,
) -> Result<Option<SegmentStats>, String> {
    ensure_local_window(&webview, "connection")?;
    Ok(state.0.lock().map_err(|e| e.to_string())?.clone())
}

#[tauri::command]
async fn close_local_window(webview: tauri::Webview) -> Result<(), String> {
    if !matches!(webview.label(), "settings" | "update-review" | "connection") {
        return Err("Недопустимая панель".into());
    }
    ensure_local_window(&webview, webview.label())?;
    let app = webview.app_handle();
    let state = app.state::<local_overlay::OverlayState>();
    let _guard = state.operation.lock().await;
    local_overlay::close(app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Register before other plugins and setup so duplicate launches do not
        // create webviews or run startup side effects.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_window("main").or_else(|| app.get_window("content")) {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .manage(BrowserState {
            url: Mutex::new("https://anivox.fun/".parse().unwrap()),
            fullscreen: std::sync::atomic::AtomicBool::new(false),
        })
        .manage(DiscordState(Mutex::new(None)))
        .manage(ShikimoriState::default())
        .manage(ShikimoriInfoState::default())
        .manage(player_settings::PlayerSettingsState::default())
        .manage(TrayState::default())
        .manage(mpv::MpvState::default())
        .manage(updater::UpdateState::default())
        .manage(site_recovery::SiteLoadState::default())
        .manage(local_overlay::OverlayState::default())
        .manage(SegmentStatsState::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let startup_url = session::initialize(app.handle())?;
            *app.state::<BrowserState>().url.lock().unwrap() = startup_url.clone();
            site_recovery::begin(app.handle());
            // All traffic goes completely through system network settings (system DNS, routes, etc.)
            #[cfg(target_os = "windows")]
            const SCRIPT: &str = concat!(
                include_str!("../scripts/inject-windows.js"),
                "\n",
                include_str!("../scripts/mpv-session.js"),
                "\n",
                include_str!("../scripts/inject-common.js"),
                "\n",
                include_str!("../scripts/player-fullscreen.js"),
                "\n",
                include_str!("../scripts/common-styles.js")
            );

            #[cfg(target_os = "linux")]
            const SCRIPT: &str = concat!(
                include_str!("../scripts/inject-linux.js"),
                "\n",
                include_str!("../scripts/mpv-session.js"),
                "\n",
                include_str!("../scripts/inject-common.js"),
                "\n",
                include_str!("../scripts/player-fullscreen.js"),
                "\n",
                include_str!("../scripts/common-styles.js")
            );

            #[cfg(not(any(target_os = "windows", target_os = "linux")))]
            const SCRIPT: &str = concat!(
                include_str!("../scripts/mpv-session.js"),
                "\n",
                include_str!("../scripts/inject-common.js"),
                "\n",
                include_str!("../scripts/player-fullscreen.js"),
                "\n",
                include_str!("../scripts/common-styles.js")
            );

            let restore_guard = if startup_url.as_str() != session::HOME_URL {
                let token = format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
                format!(";if ({}.includes(location.origin) && sessionStorage.getItem('anivox-restore-unlocked') !== '{token}') {{ window.__anivoxRestoreToken = '{token}'; {} }}", site_hosts::js_origins(), include_str!("../scripts/restore-paused.js"))
            } else { String::new() };
            let script_storage = format!("{}\n{}\n{}\n{}", restore_guard, SCRIPT, include_str!("../scripts/site-lifecycle.js"), include_str!("../scripts/segment-monitor.js"));
            let script = script_storage.as_str();

            #[cfg(target_os = "linux")]
            let script_storage = add_linux_toolbar_script(script);
            #[cfg(target_os = "linux")]
            let script = script_storage.as_str();

            #[cfg(target_os = "windows")]
            std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", BROWSER_ARGS);

            // #[cfg(target_os = "linux")]
            // if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            //     std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            // }

            #[cfg(target_os = "linux")]
            if std::env::var_os("WEBKIT_FORCE_COMPOSITING_MODE").is_none() {
                std::env::set_var("WEBKIT_FORCE_COMPOSITING_MODE", "1");
            }

            #[cfg(target_os = "linux")]
            {
                let navigation_app = app.handle().clone();
                tauri::WebviewWindowBuilder::new(
                    app,
                    "content",
                    tauri::WebviewUrl::External(startup_url.clone()),
                )
                .title("Anivox")
                .inner_size(1280.0, 720.0)
                .min_inner_size(420.0, 300.0)
                .devtools(true)
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0")
                .initialization_script(script)
                .on_navigation(move |url| {
                    if !matches!(url.scheme(), "https" | "http") {
                        return false;
                    }
                    site_recovery::begin(&navigation_app);
                    if let Ok(mut last_url) = navigation_app.state::<BrowserState>().url.lock() {
                        *last_url = url.clone();
                    }
                    true
                })
                .on_page_load(handle_content_window_load)
                .build()?;
                if let Some(window) = app.get_window("content") { if let Err(err) = session::restore_geometry(&window) { eprintln!("Не удалось восстановить окно: {err}"); } }
            }

            #[cfg(not(target_os = "linux"))]
            {
                let browser_args = browser_args_for_platform();
                let window = tauri::window::WindowBuilder::new(app, "main")
                    .title("Anivox")
                    .inner_size(1280.0, 720.0)
                    .min_inner_size(420.0, 300.0)
                    .build()?;
                if let Err(err) = session::restore_geometry(&window) { eprintln!("Не удалось восстановить окно: {err}"); }
                #[cfg(target_os = "windows")]
                set_windows_taskbar_icon(&window)?;
                window.add_child(
                    tauri::webview::WebviewBuilder::new("toolbar", tauri::WebviewUrl::App("index.html".into()))
                        .additional_browser_args(browser_args),
                    tauri::LogicalPosition::new(0.0, 0.0),
                    tauri::LogicalSize::new(1280.0, TOOLBAR_HEIGHT),
                )?;
                let navigation_app = app.handle().clone();
                window.add_child(
                    tauri::webview::WebviewBuilder::new(
                        "content",
                        tauri::WebviewUrl::External(startup_url.clone()),
                    )
                    .initialization_script(script)
                    .additional_browser_args(browser_args)
                    .on_navigation(move |url| {
                        if !matches!(url.scheme(), "https" | "http") { return false; }
                        site_recovery::begin(&navigation_app);
                    if let Ok(mut last_url) = navigation_app.state::<BrowserState>().url.lock() {
                            *last_url = url.clone();
                        }
                        true
                    })
                    .on_page_load(handle_content_load),
                    tauri::LogicalPosition::new(0.0, TOOLBAR_HEIGHT),
                    tauri::LogicalSize::new(1280.0, 720.0 - TOOLBAR_HEIGHT),
                )?;
                layout_webviews(&window)?;
            }

            let recovery_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = site_recovery::create(&recovery_app).await {
                    eprintln!("Не удалось создать экран восстановления: {err}");
                }
                // Прогреваем настройки: кеш в памяти и скрытая панель, чтобы открытие было мгновенным.
                let _ = player_settings::cached_player_settings(&recovery_app);
                if let Err(err) = local_overlay::prewarm(&recovery_app).await {
                    eprintln!("Не удалось подготовить панель настроек: {err}");
                }
            });

            let handle = app.handle().clone();
            // Initialize Discord RPC on startup
            tauri::async_runtime::spawn_blocking(move || {
                let state = handle.state::<DiscordState>();
                let mut client = state.inner().0.lock().unwrap();
                let _ = update_discord_presence(&mut client, &DiscordPresence {
                    details: "Anivox".into(),
                    state_text: "На главной".into(),
                    ..Default::default()
                });
            });

            // Check for application updates in the background 3 seconds after launch
            let updater_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                if let Ok(updater) = updater_handle.updater() {
                    if let Ok(Some(update)) = updater.check().await {
                        let _ = updater_handle.emit("update-available", serde_json::json!({
                            "version": update.version,
                            "body": update.body
                        }));
                    }
                }
            });

            let quit_item = tauri::menu::MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
            let show_item = tauri::menu::MenuItem::with_id(app, "show", "Открыть", true, None::<&str>)?;
            let tray_settings_path = app.path().app_config_dir()?.join("tray.json");
            let minimize_to_tray = tray_settings::load(&tray_settings_path).unwrap_or_else(|err| {
                eprintln!("Не удалось загрузить настройку трея: {err}");
                true
            });
            app.state::<TrayState>().0.store(minimize_to_tray, Ordering::Relaxed);
            let minimize_item = tauri::menu::CheckMenuItem::with_id(
                app, "minimize_to_tray", "Сворачивать в трей", true, minimize_to_tray, None::<&str>,
            )?;
            let menu = tauri::menu::Menu::with_items(app, &[&show_item, &minimize_item, &quit_item])?;

            let mut tray_builder = tauri::tray::TrayIconBuilder::new()
                .tooltip("Anivox")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "minimize_to_tray" => {
                        let previous = app.state::<TrayState>().0.load(Ordering::Relaxed);
                        let enabled = !previous;
                        if let Err(err) = tray_settings::save(&tray_settings_path, enabled) {
                            eprintln!("Не удалось сохранить настройку трея: {err}");
                            let _ = minimize_item.set_checked(previous);
                        } else {
                            app.state::<TrayState>().0.store(enabled, Ordering::Relaxed);
                            let _ = minimize_item.set_checked(enabled);
                        }
                    }
                    "show" => {
                        if let Some(w) = app.get_window("main").or_else(|| app.get_window("content")) {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => {
                        if let Some(window) = app.get_window("main").or_else(|| app.get_window("content")) { if let Err(err) = session::capture_geometry(&window) { eprintln!("Не удалось запомнить окно: {err}"); } }
                        if let Err(err) = session::flush(app) { eprintln!("Не удалось сохранить сеанс: {err}"); }
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_window("main").or_else(|| app.get_window("content")) {
                            let is_visible = w.is_visible().unwrap_or(false);
                            if is_visible {
                                let _ = w.hide();
                            } else {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                    }
                });

            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }

            tray_builder.build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if !is_primary_window(window.label()) { return; }
            match event {
            tauri::WindowEvent::Moved(_) => site_recovery::schedule_geometry_save(window),
            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } => {
                site_recovery::schedule_geometry_save(window);
                let window = window.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let _ = layout_webviews(&window);
                });
            }
            tauri::WindowEvent::CloseRequested { api, .. } => {
                if let Err(err) = session::capture_geometry(window) { eprintln!("Не удалось запомнить окно: {err}"); }
                if let Err(err) = session::flush(window.app_handle()) { eprintln!("Не удалось сохранить сеанс: {err}"); }
                if window.app_handle().state::<TrayState>().0.load(Ordering::Relaxed) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            tauri::WindowEvent::Destroyed => {
                window.app_handle().exit(0);
            }
            _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            discord::set_discord_rpc,
            browser_action,
            site_recovery::get_site_load_state,
            site_recovery::site_recovery_action,
            site_recovery::site_document_state,
            site_recovery::get_site_document_generation,
            session::get_session_settings,
            session::save_session_settings,
            report_player_status,
            content_fullscreen,
            mpv::open_in_mpv,
            mpv::get_mpv_state,
            mpv::get_playback_resume,
            mpv::clear_playback_history,
            open_settings,
            open_connection,
            report_segment_stats,
            get_segment_stats,
            player_settings::get_player_settings,
            player_settings::save_player_settings_command,
            player_settings::pick_mpv_executable,
            close_local_window,
            local_overlay::local_overlay_ready,
            updater::check_update,
            updater::open_update_review,
            updater::install_update,
            get_app_version,
            shikimori_action,
            close_shikimori
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
