// The experimental macOS binary has a separate minimal entry point.
#![cfg(not(target_os = "macos"))]

mod discord;
#[cfg(test)]
use discord::{build_discord_activity, send_discord_activity};
use discord::{update_discord_presence, DiscordPresence};
use discord_rich_presence::DiscordIpcClient;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
#[cfg(target_os = "linux")]
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

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
fn layout_webviews(_window: &tauri::Window) -> tauri::Result<()> {
    // Linux uses one webview with an injected toolbar; it fills the window itself.
    Ok(())
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
    layout_shikimori(window, top)?;
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
                    tauri::WebviewUrl::External("https://shikimori.io/".parse().unwrap()),
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

// State to hold the Discord IPC client
pub struct DiscordState(pub Mutex<Option<DiscordIpcClient>>);

const VPN_CONN_NAME: &str = "ANIVOX-UA-48";
const VPN_CONFIG: &str = include_str!("../../ANIVOX-UA-48.conf");

fn ensure_config_file() -> Result<std::path::PathBuf, String> {
    let temp_path = std::env::temp_dir().join("ANIVOX-UA-48.conf");
    std::fs::write(&temp_path, VPN_CONFIG).map_err(|e| format!("Failed to write config: {}", e))?;
    Ok(temp_path)
}

fn is_vpn_active() -> bool {
    #[cfg(target_os = "linux")]
    {
        if std::path::Path::new(&format!("/sys/class/net/{}", VPN_CONN_NAME)).exists() {
            return true;
        }
        if let Ok(output) = Command::new("nmcli")
            .args(["-t", "-f", "NAME", "connection", "show", "--active"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if line.trim() == VPN_CONN_NAME {
                        return true;
                    }
                }
            }
        }
        false
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let service_name = format!("WireGuardTunnel${}", VPN_CONN_NAME);
        let mut cmd = Command::new("sc.exe");
        cmd.args(["query", &service_name]);
        cmd.creation_flags(0x08000000);
        if let Ok(output) = cmd.output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("RUNNING") {
                return true;
            }
        }
        false
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        false
    }
}

#[cfg(target_os = "windows")]
fn run_wireguard(argument: &str, value: &str, allow_elevation: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let executable = r"C:\Program Files\WireGuard\wireguard.exe";
    let executable = if std::path::Path::new(executable).exists() {
        executable
    } else {
        "wireguard.exe"
    };
    let output = Command::new(executable)
        .args([argument, value])
        .creation_flags(0x08000000)
        .output()
        .map_err(|err| format!("Не удалось запустить WireGuard: {err}"))?;
    if output.status.success() {
        return Ok(());
    }

    let error = String::from_utf8_lossy(&output.stderr);
    let lower = error.to_lowercase();
    let access_denied = output.status.code() == Some(5)
        || lower.contains("access is denied")
        || lower.contains("access denied")
        || lower.contains("отказано в доступе")
        || lower.contains("доступ запрещен");
    if !access_denied || !allow_elevation {
        return Err(format!("WireGuard: {}", error.trim()));
    }

    // Only the WireGuard operation is elevated. No remote content runs as administrator.
    // Escape PowerShell literals; quote the config path separately for Windows argv parsing.
    let executable = executable.replace('\'', "''");
    let arguments = format!("{} \"{}\"", argument, value).replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference = 'Stop'; try {{ \
         $p = Start-Process -FilePath '{executable}' -ArgumentList '{arguments}' \
         -Verb RunAs -Wait -PassThru; exit $p.ExitCode \
         }} catch {{ \
         $e = $_.Exception; while ($e.InnerException) {{ $e = $e.InnerException }}; \
         if ($e.NativeErrorCode -eq 1223) {{ exit 1223 }}; \
         [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}"
    );
    let elevated = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x08000000)
        .output()
        .map_err(|err| format!("Не удалось запросить права администратора: {err}"))?;
    match elevated.status.code() {
        Some(0) => Ok(()),
        Some(1223) => Err(
            "Запрос прав администратора отменён. Для переключения VPN подтвердите запрос Windows."
                .into(),
        ),
        _ => Err(format!(
            "Не удалось переключить WireGuard с правами администратора (код {:?}). {}",
            elevated.status.code(),
            String::from_utf8_lossy(&elevated.stderr).trim()
        )),
    }
}

fn connect_vpn() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if Command::new("nmcli").arg("--version").output().is_ok() {
            let conn_exists = Command::new("nmcli")
                .args(["-t", "-f", "NAME", "connection", "show"])
                .output()
                .map(|out| {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    stdout.lines().any(|l| l.trim() == VPN_CONN_NAME)
                })
                .unwrap_or(false);

            if !conn_exists {
                let conf_path = ensure_config_file()?;
                let _ = Command::new("nmcli")
                    .args(["connection", "import", "type", "wireguard", "file"])
                    .arg(&conf_path)
                    .output();
            }

            let up_res = Command::new("nmcli")
                .args(["connection", "up", VPN_CONN_NAME])
                .output()
                .map_err(|e| format!("Failed to run nmcli up: {}", e))?;

            if !up_res.status.success() {
                let err = String::from_utf8_lossy(&up_res.stderr);
                return Err(format!("nmcli up failed: {}", err.trim()));
            }
            return Ok(());
        }

        let conf_path = ensure_config_file()?;
        let res = Command::new("wg-quick")
            .arg("up")
            .arg(&conf_path)
            .output()
            .map_err(|e| format!("wg-quick up failed: {}", e))?;

        if !res.status.success() {
            let err = String::from_utf8_lossy(&res.stderr);
            return Err(format!("wg-quick up failed: {}", err.trim()));
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        let conf_path = ensure_config_file()?;
        run_wireguard("/installtunnelservice", &conf_path.to_string_lossy(), true)?;

        // Wait up to 3 seconds for the service to become RUNNING
        for _ in 0..15 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            if is_vpn_active() {
                return Ok(());
            }
        }

        Err("Служба WireGuard не запустилась за отведённое время.".into())
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err("Unsupported OS for WireGuard VPN".to_string())
    }
}

fn disconnect_vpn(_allow_elevation: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if Command::new("nmcli").arg("--version").output().is_ok() {
            let down_res = Command::new("nmcli")
                .args(["connection", "down", VPN_CONN_NAME])
                .output()
                .map_err(|e| format!("Failed to run nmcli down: {}", e))?;

            if !down_res.status.success() {
                let err = String::from_utf8_lossy(&down_res.stderr);
                return Err(format!("nmcli down failed: {}", err.trim()));
            }
            return Ok(());
        }

        let conf_path = ensure_config_file()?;
        let res = Command::new("wg-quick")
            .arg("down")
            .arg(&conf_path)
            .output()
            .map_err(|e| format!("wg-quick down failed: {}", e))?;

        if !res.status.success() {
            let err = String::from_utf8_lossy(&res.stderr);
            return Err(format!("wg-quick down failed: {}", err.trim()));
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        run_wireguard("/uninstalltunnelservice", VPN_CONN_NAME, _allow_elevation)?;

        // Wait for service to stop
        for _ in 0..15 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            if !is_vpn_active() {
                return Ok(());
            }
        }

        Err("Служба WireGuard не остановилась за отведённое время.".into())
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err("Unsupported OS for WireGuard VPN".to_string())
    }
}

#[tauri::command]
async fn get_vpn_status() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(is_vpn_active)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn toggle_vpn(webview: tauri::Webview, enable: Option<bool>) -> Result<bool, String> {
    ensure_toolbar_access(&webview)?;
    tauri::async_runtime::spawn_blocking(move || toggle_vpn_blocking(enable))
        .await
        .map_err(|err| err.to_string())?
}

fn toggle_vpn_blocking(enable: Option<bool>) -> Result<bool, String> {
    let current = is_vpn_active();
    let target = match enable {
        Some(val) => val,
        None => !current,
    };

    if target == current {
        return Ok(current);
    }

    if target {
        connect_vpn()?;
    } else {
        disconnect_vpn(true)?;
    }

    Ok(is_vpn_active())
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

#[tauri::command]
async fn set_discord_rpc(
    app: tauri::AppHandle,
    mut presence: DiscordPresence,
) -> Result<(), String> {
    presence.details = presence.details.chars().take(128).collect();
    presence.state_text = presence.state_text.chars().take(128).collect();
    // Discord's handshake performs blocking pipe/socket I/O without a timeout.
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<DiscordState>();
        let mut client = state.inner().0.lock().map_err(|e| e.to_string())?;
        let mpv = app.state::<MpvState>();
        *mpv.embedded.lock().map_err(|e| e.to_string())? = presence.clone();
        if mpv
            .current
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .is_some_and(|s| s.playback.ready)
        {
            return Ok(());
        }
        update_discord_presence(&mut client, &presence)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
#[serde(default, rename_all = "camelCase")]
struct PlayerSettings {
    mpv_path: Option<String>,
    mirror_embedded: bool,
}

#[derive(Default)]
struct PlayerSettingsState(Mutex<()>);

fn validate_mpv_path(value: Option<String>) -> Result<Option<String>, String> {
    let value = value.unwrap_or_default().trim().to_owned();
    if value.is_empty() {
        return Ok(None);
    }
    let path = Path::new(&value);
    if !path.is_absolute() || !path.is_file() {
        return Err("Укажите полный путь к существующему исполняемому файлу MPV".into());
    }
    Ok(Some(value))
}

fn load_player_settings(path: &Path) -> Result<PlayerSettings, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|err| format!("Не удалось прочитать settings.json: {err}")),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(PlayerSettings::default()),
        Err(err) => Err(format!("Не удалось открыть настройки: {err}")),
    }
}

fn save_player_settings(path: &Path, settings: &PlayerSettings) -> Result<(), String> {
    use std::io::Write;
    let settings = PlayerSettings {
        mpv_path: validate_mpv_path(settings.mpv_path.clone())?,
        mirror_embedded: settings.mirror_embedded,
    };
    let parent = path.parent().ok_or("Не определён каталог настроек")?;
    std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|err| err.to_string())?;
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|err| err.to_string())?;
    file.write_all(&bytes).map_err(|err| err.to_string())?;
    file.as_file().sync_all().map_err(|err| err.to_string())?;
    file.persist(path)
        .map_err(|err| format!("Не удалось сохранить настройки: {}", err.error))?;
    Ok(())
}

fn player_settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|err| err.to_string())?
        .join("settings.json"))
}

fn player_logs_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|err| err.to_string())?
        .join("logs"))
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

fn open_local_window(
    app: &tauri::AppHandle,
    label: &str,
    page: &str,
    title: &str,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(label) {
        // A second click while the page is loading must not reveal about:blank.
        if !window.is_visible().map_err(|err| err.to_string())? {
            return Ok(());
        }
        window.show().map_err(|err| err.to_string())?;
        window.unminimize().map_err(|err| err.to_string())?;
        return window.set_focus().map_err(|err| err.to_string());
    }
    let navigation_dev_url = app.config().build.dev_url.clone();
    let loaded_dev_url = navigation_dev_url.clone();
    tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App(page.into()))
        .title(title)
        .inner_size(560.0, 420.0)
        .min_inner_size(420.0, 320.0)
        // New WebView2 controllers initially show about:blank. Reveal the
        // auxiliary window only after its local document has finished loading.
        .visible(false)
        .additional_browser_args(browser_args_for_platform())
        .on_navigation(move |url| is_local_app_url(url, navigation_dev_url.as_ref()))
        .on_page_load(move |webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
                && is_local_app_url(payload.url(), loaded_dev_url.as_ref())
            {
                let _ = webview.show();
                let _ = webview.set_focus();
            }
        })
        .build()
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[tauri::command]
async fn open_settings(app: tauri::AppHandle, webview: tauri::Webview) -> Result<(), String> {
    ensure_toolbar_access(&webview)?;
    open_local_window(&app, "settings", "settings.html", "Anivox — настройки")
}

#[tauri::command]
fn close_local_window(webview: tauri::Webview) -> Result<(), String> {
    if !matches!(webview.label(), "settings" | "update-review") {
        return Err("Недопустимое окно".into());
    }
    ensure_local_window(&webview, webview.label())?;
    webview.window().destroy().map_err(|err| err.to_string())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayerSettingsInfo {
    mpv_path: Option<String>,
    log_directory: String,
    mirror_embedded: bool,
}

fn player_settings_info(
    app: &tauri::AppHandle,
    settings: PlayerSettings,
) -> Result<PlayerSettingsInfo, String> {
    Ok(PlayerSettingsInfo {
        mpv_path: settings.mpv_path,
        mirror_embedded: settings.mirror_embedded,
        log_directory: player_logs_path(app)?.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
async fn get_player_settings(
    app: tauri::AppHandle,
    webview: tauri::Webview,
) -> Result<PlayerSettingsInfo, String> {
    ensure_local_window(&webview, "settings")?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<PlayerSettingsState>();
        let _guard = state.0.lock().map_err(|err| err.to_string())?;
        player_settings_info(&app, load_player_settings(&player_settings_path(&app)?)?)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn save_player_settings_command(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    mpv_path: Option<String>,
    mirror_embedded: Option<bool>,
) -> Result<PlayerSettingsInfo, String> {
    ensure_local_window(&webview, "settings")?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<PlayerSettingsState>();
        let _guard = state.0.lock().map_err(|err| err.to_string())?;
        let settings = PlayerSettings {
            mpv_path: validate_mpv_path(mpv_path)?,
            mirror_embedded: match mirror_embedded {
                Some(value) => value,
                None => load_player_settings(&player_settings_path(&app)?)?.mirror_embedded,
            },
        };
        save_player_settings(&player_settings_path(&app)?, &settings)?;
        player_settings_info(&app, settings)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn pick_mpv_executable(
    app: tauri::AppHandle,
    webview: tauri::Webview,
) -> Result<Option<String>, String> {
    ensure_local_window(&webview, "settings")?;
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = app
            .dialog()
            .file()
            .set_title("Выберите MPV")
            .set_parent(&webview.window());
        #[cfg(target_os = "windows")]
        let dialog = dialog.add_filter("Исполняемый файл MPV", &["exe"]);
        dialog
            .blocking_pick_file()
            .map(|file| {
                file.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|err| err.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|err| err.to_string())?
}

fn build_mpv_command(
    settings: &PlayerSettings,
    log_path: &Path,
    url: String,
    title: Option<String>,
    start_time: Option<f64>,
    sub_url: Option<String>,
    token: Option<String>,
) -> Command {
    let mut cmd = Command::new(settings.mpv_path.as_deref().unwrap_or("mpv"));

    if let Some(t) = title {
        if !t.is_empty() {
            cmd.arg(format!("--force-media-title={}", t));
        }
    }

    if let Some(st) = start_time {
        if st > 0.0 {
            cmd.arg(format!("--start={:.2}", st));
        }
    }

    if let Some(sub) = sub_url {
        if !sub.is_empty() {
            cmd.arg(format!("--sub-file={}", sub));
        }
    }

    // Pass headers cleanly to MPV and FFmpeg demuxer
    cmd.arg("--referrer=https://anivox.fun/");
    cmd.arg("--http-header-fields-append=Origin: https://anivox.fun");
    if let Some(tok) = token {
        if !tok.is_empty() {
            cmd.arg(format!(
                "--http-header-fields-append=Authorization: Bearer {}",
                tok
            ));
        }
    }
    cmd.arg("--user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0");

    // UI and logging flags: show window immediately, don't exit silently on error, write debug log
    cmd.arg("--force-window=immediate");
    cmd.arg("--keep-open=yes");
    cmd.arg("--hls-bitrate=max");
    cmd.arg("--ytdl-format=bestvideo+bestaudio/best");
    let mut log_arg = std::ffi::OsString::from("--log-file=");
    log_arg.push(log_path);
    cmd.arg(log_arg);
    cmd.arg("--").arg(url);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaybackMedia {
    key: String,
    presence: DiscordPresence,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResumePoint {
    position: f64,
    duration: Option<f64>,
    updated_at: u64,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct PlaybackHistory {
    entries: std::collections::BTreeMap<String, ResumePoint>,
}

fn valid_media_key(key: &str) -> bool {
    let parts: Vec<_> = key.split(':').collect();
    key.len() <= 80
        && parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|c| c.is_ascii_digit())
                && part.parse::<u64>().is_ok_and(|n| n > 0)
        })
}

impl PlaybackHistory {
    fn record(&mut self, key: &str, position: f64, duration: Option<f64>) {
        if !valid_media_key(key)
            || !position.is_finite()
            || !(0.0..=7.0 * 86400.0).contains(&position)
        {
            return;
        }
        let duration = duration.filter(|n| n.is_finite() && *n > 0.0 && *n <= 7.0 * 86400.0);
        let updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.entries.insert(
            key.into(),
            ResumePoint {
                position,
                duration,
                updated_at,
            },
        );
        while self.entries.len() > 1000 {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, p)| p.updated_at)
                .map(|(key, _)| key.clone());
            if let Some(key) = oldest {
                self.entries.remove(&key);
            }
        }
    }
    fn resume(&self, key: &str) -> Option<ResumePoint> {
        if !valid_media_key(key) {
            return None;
        }
        self.entries
            .get(key)
            .filter(|p| {
                p.position.is_finite()
                    && p.position >= 10.0
                    && p.position <= 7.0 * 86400.0
                    && p.duration.is_none_or(|d| {
                        d.is_finite() && d > 0.0 && p.position < d * 0.95 && d - p.position > 30.0
                    })
            })
            .cloned()
    }
}

fn history_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("playback-history.json"))
}
fn read_history(path: &Path) -> Result<PlaybackHistory, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| format!("Не удалось прочитать историю: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(PlaybackHistory::default()),
        Err(e) => Err(e.to_string()),
    }
}
fn write_history(path: &Path, history: &PlaybackHistory) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().ok_or("Не найден каталог истории")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(history).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.error.to_string())?;
    Ok(())
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MpvPlayback {
    ready: bool,
    paused: bool,
    buffering: bool,
    ended: bool,
    position: Option<f64>,
    duration: Option<f64>,
    speed: f64,
}
impl Default for MpvPlayback {
    fn default() -> Self {
        Self {
            ready: false,
            paused: true,
            buffering: false,
            ended: false,
            position: None,
            duration: None,
            speed: 1.0,
        }
    }
}
impl MpvPlayback {
    fn apply(&mut self, event: &serde_json::Value) {
        match event["event"].as_str() {
            Some("file-loaded") => {
                self.ready = true;
                self.ended = false;
            }
            Some("end-file") => {
                self.ended = true;
                self.paused = true;
            }
            Some("property-change") => {
                let data = &event["data"];
                match event["name"].as_str() {
                    Some("pause") => {
                        if let Some(v) = data.as_bool() {
                            self.paused = v;
                        }
                    }
                    Some("paused-for-cache") => {
                        if let Some(v) = data.as_bool() {
                            self.buffering = v;
                        }
                    }
                    Some("eof-reached") => {
                        if let Some(ended) = data.as_bool() {
                            self.ended = ended;
                            if ended {
                                self.paused = true;
                            }
                        }
                    }
                    Some("time-pos") => {
                        if let Some(v) = data.as_f64().filter(|v| v.is_finite() && *v >= 0.0) {
                            self.position = Some(v);
                        }
                    }
                    Some("duration") => {
                        if let Some(v) = data.as_f64().filter(|v| v.is_finite() && *v > 0.0) {
                            self.duration = Some(v);
                        }
                    }
                    Some("speed") => {
                        if let Some(v) = data
                            .as_f64()
                            .filter(|v| v.is_finite() && *v > 0.0 && *v <= 16.0)
                        {
                            self.speed = v;
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    }
}
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MpvSession {
    session_id: String,
    media_key: String,
    mirror_embedded: bool,
    #[serde(flatten)]
    playback: MpvPlayback,
    #[serde(skip)]
    media: Option<PlaybackMedia>,
}
#[derive(Default)]
struct MpvState {
    current: Mutex<Option<MpvSession>>,
    embedded: Mutex<DiscordPresence>,
    history: Mutex<()>,
    progress_owners: Mutex<std::collections::BTreeMap<String, String>>,
    rpc_pending: std::sync::atomic::AtomicBool,
    launch: tokio::sync::Mutex<()>,
}
impl MpvState {
    fn owns_progress(&self, sample: &MpvSession) -> bool {
        self.progress_owners
            .lock()
            .is_ok_and(|owners| owners.get(&sample.media_key) == Some(&sample.session_id))
    }
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MpvLaunchInfo {
    session_id: String,
    mirror_embedded: bool,
}

fn ensure_content_access(webview: &tauri::Webview) -> Result<(), String> {
    if webview.label() == "content" {
        Ok(())
    } else {
        Err("Доступно только в плеере Anivox".into())
    }
}
#[tauri::command]
fn get_mpv_state(
    app: tauri::AppHandle,
    webview: tauri::Webview,
) -> Result<Option<MpvSession>, String> {
    ensure_content_access(&webview)?;
    let value = app
        .state::<MpvState>()
        .current
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    Ok(value)
}
#[tauri::command]
async fn get_playback_resume(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    key: String,
) -> Result<Option<ResumePoint>, String> {
    ensure_content_access(&webview)?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<MpvState>();
        let _guard = state.history.lock().map_err(|e| e.to_string())?;
        Ok(read_history(&history_path(&app)?)?.resume(&key))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn clear_playback_history(
    app: tauri::AppHandle,
    webview: tauri::Webview,
) -> Result<(), String> {
    ensure_local_window(&webview, "settings")?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<MpvState>();
        let _guard = state.history.lock().map_err(|e| e.to_string())?;
        write_history(&history_path(&app)?, &PlaybackHistory::default())
    })
    .await
    .map_err(|e| e.to_string())?
}
fn persist_mpv_progress(app: &tauri::AppHandle, sample: &MpvSession) -> Result<(), String> {
    let state = app.state::<MpvState>();
    let owners = state.progress_owners.lock().map_err(|e| e.to_string())?;
    if owners.get(&sample.media_key) != Some(&sample.session_id) || !sample.playback.ready {
        return Ok(());
    }
    if let Some(position) = sample.playback.position {
        if valid_media_key(&sample.media_key) {
            let state = app.state::<MpvState>();
            let _guard = state.history.lock().map_err(|e| e.to_string())?;
            let path = history_path(app)?;
            let mut history = read_history(&path)?;
            history.record(&sample.media_key, position, sample.playback.duration);
            write_history(&path, &history)?;
        }
    }
    Ok(())
}
fn mpv_presence(sample: &MpvSession) -> DiscordPresence {
    let mut presence = sample
        .media
        .as_ref()
        .map(|m| m.presence.clone())
        .unwrap_or_default();
    if presence.details.is_empty() {
        presence.details = "Anivox".into();
    }
    let status = if sample.playback.ended {
        "Просмотр завершён"
    } else if sample.playback.buffering {
        "Загрузка"
    } else if sample.playback.paused {
        "На паузе"
    } else {
        "Смотрит"
    };
    presence.state_text = format!(
        "{}{}MPV · {status}",
        presence.state_text,
        if presence.state_text.is_empty() {
            ""
        } else {
            " · "
        }
    );
    presence.state_text = presence.state_text.chars().take(128).collect();
    presence.end_timestamp = None;
    if !sample.playback.paused && !sample.playback.buffering && !sample.playback.ended {
        if let (Some(position), Some(duration)) =
            (sample.playback.position, sample.playback.duration)
        {
            let remaining = (duration - position) / sample.playback.speed;
            if remaining > 0.0 && remaining < 7.0 * 86400.0 {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                presence.end_timestamp = Some(now as i64 + remaining.ceil() as i64);
            }
        }
    }
    presence
}
fn refresh_mpv_discord(app: &tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    if app
        .state::<MpvState>()
        .rpc_pending
        .swap(true, Ordering::AcqRel)
    {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<MpvState>();
        if let Ok(mut client) = app.state::<DiscordState>().0.lock() {
            let sample = state.current.lock().ok().and_then(|s| s.clone());
            let presence = sample
                .as_ref()
                .filter(|s| s.playback.ready)
                .map(mpv_presence)
                .unwrap_or_else(|| state.embedded.lock().map(|p| p.clone()).unwrap_or_default());
            let _ = update_discord_presence(&mut client, &presence);
        }
        state.rpc_pending.store(false, Ordering::Release);
    });
}

trait MpvIo: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> MpvIo for T {}
async fn connect_mpv_ipc(endpoint: &str) -> std::io::Result<Box<dyn MpvIo>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(
            tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint)?,
        ))
    }
    #[cfg(unix)]
    {
        Ok(Box::new(tokio::net::UnixStream::connect(endpoint).await?))
    }
    #[cfg(not(any(target_os = "windows", unix)))]
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "MPV IPC unsupported",
        ))
    }
}
async fn observe_mpv(stream: &mut Box<dyn MpvIo>) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    for (id, property) in [
        "pause",
        "time-pos",
        "duration",
        "speed",
        "paused-for-cache",
        "eof-reached",
    ]
    .iter()
    .enumerate()
    {
        let command = serde_json::json!({"command":["observe_property",id+1,property]});
        stream
            .write_all(format!("{command}\n").as_bytes())
            .await
            .map_err(|e| e.to_string())?;
    }
    // Query time-pos too: file-loaded may precede the IPC connection.
    stream
        .write_all(b"{\"command\":[\"get_property\",\"time-pos\"],\"request_id\":100}\n")
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
async fn track_mpv(
    app: tauri::AppHandle,
    mut child: tokio::process::Child,
    stream: Box<dyn MpvIo>,
    mut sample: MpvSession,
    _ipc_dir: tempfile::TempDir,
) {
    use tokio::io::AsyncBufReadExt;
    let mut reader = tokio::io::BufReader::new(stream);
    let mut line = Vec::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut persisted = std::time::Instant::now();
    let started = std::time::Instant::now();
    let mut failure = None;
    loop {
        tokio::select! {
            result=reader.read_until(b'\n',&mut line)=>{
                match result {
                    Ok(0)=>{if child.try_wait().ok().flatten().is_none(){failure=Some("Соединение с MPV закрыто".to_string());}break;},
                    Ok(_) if line.len()<=65536=>{
                        if let Ok(event)=serde_json::from_slice::<serde_json::Value>(&line) {
                            sample.playback.apply(&event);
                            if event["request_id"]==100 && event["error"]=="success" && event["data"].as_f64().is_some_and(|n|n.is_finite() && n>=0.0) {sample.playback.ready=true;}
                            if sample.playback.ended && event["reason"]=="error" {failure=Some("MPV не смог открыть видео. Проверьте журнал плеера.".into());}
                        }
                        line.clear();
                    },
                    _=>{failure=Some("Ошибка чтения состояния MPV".into());break;}
                }
            },
            result=child.wait()=>{
                if !result.is_ok_and(|s|s.success()){failure=Some("MPV завершился с ошибкой. Проверьте журнал плеера.".into());}
                break;
            },
            _=tick.tick()=>{
                let owned={
                    let state=app.state::<MpvState>();
                    let mut current=state.current.lock().unwrap_or_else(|e|e.into_inner());
                    if current.as_ref().is_some_and(|s|s.session_id==sample.session_id){*current=Some(sample.clone());true}else{false}
                };
                if owned {refresh_mpv_discord(&app);}
                if persisted.elapsed().as_secs()>=5 && app.state::<MpvState>().owns_progress(&sample) {
                    if let Err(error)=persist_mpv_progress(&app,&sample){failure=Some(format!("Не удалось сохранить позицию: {error}"));}
                    persisted=std::time::Instant::now();
                }
                if !sample.playback.ready && started.elapsed().as_secs()>60 {failure=Some("MPV не подтвердил открытие видео за 60 секунд".into());break;}
            }
        }
    }
    let _ = persist_mpv_progress(&app, &sample);
    let owned = {
        let state = app.state::<MpvState>();
        let mut current = state.current.lock().unwrap_or_else(|e| e.into_inner());
        if current
            .as_ref()
            .is_some_and(|s| s.session_id == sample.session_id)
        {
            *current = None;
            true
        } else {
            false
        }
    };
    if owned {
        refresh_mpv_discord(&app);
        if let Some(error) = failure {
            let _ = report_player_status(
                app.clone(),
                "⚠ MPV".into(),
                "#ff5252".into(),
                10000,
                Some(error),
            );
        }
    }
    // Closing Anivox never kills a user's external player. Reap our child independently.
    if child.try_wait().ok().flatten().is_none() {
        tauri::async_runtime::spawn(async move {
            let _ = child.wait().await;
        });
    }
}

#[tauri::command]
async fn open_in_mpv(
    app: tauri::AppHandle,
    url: String,
    title: Option<String>,
    start_time: Option<f64>,
    sub_url: Option<String>,
    token: Option<String>,
    media: Option<PlaybackMedia>,
) -> Result<MpvLaunchInfo, String> {
    let state = app.state::<MpvState>();
    let _launch = state.launch.try_lock().map_err(|_| "MPV уже запускается")?;
    let app_copy = app.clone();
    let (command, session_id, mirror_embedded, ipc_dir, endpoint) =
        tauri::async_runtime::spawn_blocking(move || {
            let settings = {
                let state = app_copy.state::<PlayerSettingsState>();
                let _guard = state.0.lock().map_err(|e| e.to_string())?;
                load_player_settings(&player_settings_path(&app_copy)?)?
            };
            let settings = PlayerSettings {
                mpv_path: validate_mpv_path(settings.mpv_path)?,
                ..settings
            };
            let logs = player_logs_path(&app_copy)?;
            std::fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let session_id = format!("{}-{stamp}", std::process::id());
            let ipc_dir = tempfile::Builder::new()
                .prefix("anivox-mpv-")
                .tempdir()
                .map_err(|e| e.to_string())?;
            #[cfg(target_os = "windows")]
            let endpoint = format!(r"\\.\pipe\anivox-mpv-{session_id}");
            #[cfg(not(target_os = "windows"))]
            let endpoint = ipc_dir.path().join("ipc").to_string_lossy().into_owned();
            let mut command = build_mpv_command(
                &settings,
                &logs.join(format!("mpv-{session_id}.log")),
                url,
                title,
                start_time,
                sub_url,
                token,
            );
            // Insert before the option terminator; URLs always remain positional arguments.
            let args: Vec<_> = command.get_args().map(|s| s.to_os_string()).collect();
            let mut ipc_command = Command::new(command.get_program());
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                ipc_command.creation_flags(0x08000000);
            }
            ipc_command.arg(format!("--input-ipc-server={endpoint}"));
            ipc_command.args(args);
            command = ipc_command;
            Ok::<_, String>((
                command,
                session_id,
                settings.mirror_embedded,
                ipc_dir,
                endpoint,
            ))
        })
        .await
        .map_err(|e| e.to_string())??;
    let mut child = tokio::process::Command::from(command)
        .spawn()
        .map_err(|e| format!("Не удалось запустить MPV: {e}. Выберите плеер в настройках."))?;
    let mut connected = None;
    for _ in 0..80 {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return Err("MPV завершился до подключения IPC. Проверьте журнал плеера.".into());
        }
        match connect_mpv_ipc(&endpoint).await {
            Ok(stream) => {
                connected = Some(stream);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
        }
    }
    let Some(mut stream) = connected else {
        tauri::async_runtime::spawn(async move {
            let _ = child.wait().await;
        });
        return Err("MPV запущен, но IPC недоступен. Встроенный плеер не изменён.".into());
    };
    observe_mpv(&mut stream).await?;
    let media = media.map(|mut m| {
        m.key = if valid_media_key(&m.key) {
            m.key
        } else {
            String::new()
        };
        m.presence.details = m.presence.details.chars().take(128).collect();
        m.presence.state_text = m.presence.state_text.chars().take(80).collect();
        m
    });
    let sample = MpvSession {
        session_id: session_id.clone(),
        media_key: media.as_ref().map(|m| m.key.clone()).unwrap_or_default(),
        mirror_embedded,
        playback: MpvPlayback::default(),
        media,
    };
    if valid_media_key(&sample.media_key) {
        let mut owners = state.progress_owners.lock().map_err(|e| e.to_string())?;
        owners.insert(sample.media_key.clone(), sample.session_id.clone());
        while owners.len() > 1000 {
            let key = owners.keys().find(|key| *key != &sample.media_key).cloned();
            if let Some(key) = key {
                owners.remove(&key);
            } else {
                break;
            }
        }
    }
    *state.current.lock().map_err(|e| e.to_string())? = Some(sample.clone());
    tauri::async_runtime::spawn(track_mpv(app.clone(), child, stream, sample, ipc_dir));
    Ok(MpvLaunchInfo {
        session_id,
        mirror_embedded,
    })
}

#[derive(serde::Serialize, Clone)]
struct UpdateInfo {
    available: bool,
    version: Option<String>,
    body: Option<String>,
}

#[derive(Default)]
struct UpdateState {
    operation: tokio::sync::Mutex<()>,
}

#[tauri::command]
async fn open_update_review(app: tauri::AppHandle, webview: tauri::Webview) -> Result<(), String> {
    ensure_toolbar_access(&webview)?;
    open_local_window(
        &app,
        "update-review",
        "update-review.html",
        "Anivox — обновление",
    )
}

#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> Result<UpdateInfo, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(UpdateInfo {
            available: true,
            version: Some(update.version),
            body: update.body,
        }),
        Ok(None) => Ok(UpdateInfo {
            available: false,
            version: None,
            body: None,
        }),
        Err(e) => Err(e.to_string()),
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum UpdateInstallOutcome {
    #[cfg(target_os = "linux")]
    ReleasePage,
    NoUpdate,
    UpdateChanged {
        version: String,
        body: Option<String>,
    },
}

fn changed_update_review(
    expected: &str,
    version: &str,
    body: Option<String>,
) -> Option<UpdateInstallOutcome> {
    (expected != version).then(|| UpdateInstallOutcome::UpdateChanged {
        version: version.to_owned(),
        body,
    })
}

#[tauri::command]
async fn install_update(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    expected_version: String,
) -> Result<UpdateInstallOutcome, String> {
    ensure_local_window(&webview, "update-review")?;
    let state = app.state::<UpdateState>();
    let _operation = state
        .operation
        .try_lock()
        .map_err(|_| "Обновление уже устанавливается")?;
    let result = install_reviewed_update(&app, &expected_version).await;
    let _ = app.emit("update-idle", ());
    result
}

async fn install_reviewed_update(
    app: &tauri::AppHandle,
    expected_version: &str,
) -> Result<UpdateInstallOutcome, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    if let Some(update) = update {
        if let Some(changed) =
            changed_update_review(expected_version, &update.version, update.body.clone())
        {
            return Ok(changed);
        }
        #[cfg(target_os = "linux")]
        if std::env::var_os("APPIMAGE").is_none() {
            app.opener()
                .open_url(
                    "https://github.com/AXELL2022/Anivox-Wraper/releases/latest",
                    None::<&str>,
                )
                .map_err(|err| err.to_string())?;
            return Ok(UpdateInstallOutcome::ReleasePage);
        }
        let _ = app.emit("update-started", ());
        let mut downloaded: usize = 0;
        let app_progress = app.clone();
        let app_finished = app.clone();
        update
            .download_and_install(
                move |chunk_length, content_length| {
                    downloaded += chunk_length;
                    let percent = content_length
                        .map(|total| {
                            if total > 0 {
                                ((downloaded as f64 / total as f64) * 100.0).min(100.0) as u32
                            } else {
                                0
                            }
                        })
                        .unwrap_or(0);

                    let _ = app_progress.emit(
                        "update-progress",
                        serde_json::json!({
                            "downloaded": downloaded,
                            "total": content_length,
                            "percent": percent
                        }),
                    );
                },
                move || {
                    let _ = app_finished.emit("update-finished", ());
                },
            )
            .await
            .map_err(|e| e.to_string())?;

        app.restart();
    }
    Ok(UpdateInstallOutcome::NoUpdate)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Register before other plugins and setup so duplicate launches do not
        // create webviews or run startup side effects (including VPN cleanup).
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
        .manage(PlayerSettingsState::default())
        .manage(MpvState::default())
        .manage(UpdateState::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // All traffic goes completely through system network settings (system DNS, routes, etc.)
            #[cfg(target_os = "windows")]
            const SCRIPT: &str = concat!(
                include_str!("../scripts/inject-windows.js"),
                "\n",
                include_str!("../scripts/mpv-session.js"),
                "\n",
                include_str!("../scripts/inject-common.js")
            );

            #[cfg(target_os = "linux")]
            const SCRIPT: &str = concat!(
                include_str!("../scripts/inject-linux.js"),
                "\n",
                include_str!("../scripts/mpv-session.js"),
                "\n",
                include_str!("../scripts/inject-common.js")
            );

            #[cfg(not(any(target_os = "windows", target_os = "linux")))]
            const SCRIPT: &str = concat!(include_str!("../scripts/mpv-session.js"), "\n", include_str!("../scripts/inject-common.js"));

            let script = SCRIPT;

            #[cfg(target_os = "linux")]
            let script_storage = add_linux_toolbar_script(script);
            #[cfg(target_os = "linux")]
            let script = script_storage.as_str();

            // Ensure WireGuard VPN is OFF by default on app launch
            if is_vpn_active() {
                let _ = disconnect_vpn(false);
            }

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
                    tauri::WebviewUrl::External("https://anivox.fun/".parse().unwrap()),
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
                    if let Ok(mut last_url) = navigation_app.state::<BrowserState>().url.lock() {
                        *last_url = url.clone();
                    }
                    true
                })
                .build()?;
            }

            #[cfg(not(target_os = "linux"))]
            {
                let browser_args = browser_args_for_platform();
                let window = tauri::window::WindowBuilder::new(app, "main")
                    .title("Anivox")
                    .inner_size(1280.0, 720.0)
                    .min_inner_size(420.0, 300.0)
                    .build()?;
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
                        tauri::WebviewUrl::External("https://anivox.fun/".parse().unwrap()),
                    )
                    .initialization_script(script)
                    .additional_browser_args(browser_args)
                    .on_navigation(move |url| {
                        if !matches!(url.scheme(), "https" | "http") { return false; }
                        if let Ok(mut last_url) = navigation_app.state::<BrowserState>().url.lock() {
                            *last_url = url.clone();
                        }
                        true
                    })
                    .on_page_load(|webview, payload| {
                        if matches!(payload.event(), tauri::webview::PageLoadEvent::Started) {
                            tauri::async_runtime::spawn(async move {
                                let _ = content_fullscreen(webview.window(), false).await;
                            });
                        }
                    }),
                    tauri::LogicalPosition::new(0.0, TOOLBAR_HEIGHT),
                    tauri::LogicalSize::new(1280.0, 720.0 - TOOLBAR_HEIGHT),
                )?;
                layout_webviews(&window)?;
            }

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
            let menu = tauri::menu::Menu::with_items(app, &[&show_item, &quit_item])?;

            let mut tray_builder = tauri::tray::TrayIconBuilder::new()
                .tooltip("Anivox")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(w) = app.get_window("main").or_else(|| app.get_window("content")) {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => {
                        if is_vpn_active() {
                            let _ = disconnect_vpn(false);
                        }
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
            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } => {
                let window = window.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let _ = layout_webviews(&window);
                });
            }
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            tauri::WindowEvent::Destroyed => {
                if is_vpn_active() {
                    let _ = disconnect_vpn(false);
                }
                window.app_handle().exit(0);
            }
            _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            set_discord_rpc,
            get_vpn_status,
            toggle_vpn,
            browser_action,
            report_player_status,
            content_fullscreen,
            open_in_mpv,
            get_mpv_state,
            get_playback_resume,
            clear_playback_history,
            open_settings,
            get_player_settings,
            save_player_settings_command,
            pick_mpv_executable,
            close_local_window,
            check_update,
            open_update_review,
            install_update,
            get_app_version,
            shikimori_action,
            close_shikimori
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
