use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::process::Command;
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};
#[cfg(target_os = "linux")]
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

const TOOLBAR_HEIGHT: f64 = 40.0;
const SHIKIMORI_MARGIN: f64 = 16.0;
const SHIKIMORI_HEADER: f64 = 40.0;

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
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let target = "content";
    #[cfg(not(target_os = "linux"))]
    let target = "toolbar";
    app.emit_to(
        target,
        "player-status",
        serde_json::json!({
            "text": text, "color": color, "resetAfterMs": reset_after_ms
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
fn set_discord_rpc(
    state: State<'_, DiscordState>,
    details: Option<String>,
    state_text: Option<String>,
) -> Result<(), String> {
    let discord_state: &DiscordState = state.inner();
    let mut client_guard = discord_state.0.lock().unwrap();

    if let Some(client) = client_guard.as_mut() {
        let mut activity = activity::Activity::new();

        // We need to keep the strings alive if we are going to use them in the activity
        let details = details.unwrap_or_default();
        let state_text = state_text.unwrap_or_default();

        if !details.is_empty() {
            activity = activity.details(&details);
        }
        if !state_text.is_empty() {
            activity = activity.state(&state_text);
        }

        activity = activity.assets(
            activity::Assets::new()
                .large_image("logo")
                .large_text("Anivox"),
        );

        client.set_activity(activity).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("Discord client not connected".to_string())
    }
}

#[tauri::command]
fn open_in_mpv(
    url: String,
    title: Option<String>,
    start_time: Option<f64>,
    sub_url: Option<String>,
    token: Option<String>,
) -> Result<(), String> {
    println!("[MPV] Launch requested for URL: {}", url);
    let mpv_exe = std::path::Path::new(r"F:\Mpv\mpv.exe");
    let (mpv_path, working_dir) = if mpv_exe.exists() {
        (r"F:\Mpv\mpv.exe".to_string(), Some(r"F:\Mpv"))
    } else {
        ("mpv".to_string(), None)
    };

    let mut cmd = std::process::Command::new(&mpv_path);
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    cmd.arg(&url);

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
            cmd.arg(format!("--http-header-fields-append=Authorization: Bearer {}", tok));
        }
    }
    cmd.arg("--user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0");

    // UI and logging flags: show window immediately, don't exit silently on error, write debug log
    cmd.arg("--force-window=immediate");
    cmd.arg("--keep-open=yes");
    cmd.arg("--hls-bitrate=max");
    cmd.arg("--ytdl-format=bestvideo+bestaudio/best");
    cmd.arg(r"--log-file=F:\Mpv\mpv_debug.log");

    match cmd.spawn() {
        Ok(child) => {
            println!("[MPV] Successfully started MPV (PID: {})", child.id());
            Ok(())
        }
        Err(e) => {
            let msg = format!("Failed to launch MPV at {}: {}", mpv_path, e);
            eprintln!("[MPV Error] {}", msg);
            Err(msg)
        }
    }
}

#[derive(serde::Serialize, Clone)]
struct UpdateInfo {
    available: bool,
    version: Option<String>,
    body: Option<String>,
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

#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_none() {
        let _ = app.opener().open_url("https://github.com/AXELL2022/Anivox-Wraper/releases/latest", None::<&str>);
        return Ok(());
    }

    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    if let Some(update) = update {
        let mut downloaded: usize = 0;
        let app_progress = app.clone();
        let app_finished = app.clone();
        update.download_and_install(
            move |chunk_length, content_length| {
                downloaded += chunk_length;
                let percent = content_length.map(|total| {
                    if total > 0 {
                        ((downloaded as f64 / total as f64) * 100.0).min(100.0) as u32
                    } else {
                        0
                    }
                }).unwrap_or(0);

                let _ = app_progress.emit("update-progress", serde_json::json!({
                    "downloaded": downloaded,
                    "total": content_length,
                    "percent": percent
                }));
            },
            move || {
                let _ = app_finished.emit("update-finished", ());
            }
        ).await.map_err(|e| e.to_string())?;

        app.restart();
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(BrowserState {
            url: Mutex::new("https://anivox.fun/".parse().unwrap()),
            fullscreen: std::sync::atomic::AtomicBool::new(false),
        })
        .manage(DiscordState(Mutex::new(None)))
        .manage(ShikimoriState::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // All traffic goes completely through system network settings (system DNS, routes, etc.)
            #[cfg(target_os = "windows")]
            const SCRIPT: &str = concat!(
                include_str!("../scripts/inject-windows.js"),
                "\n",
                include_str!("../scripts/inject-common.js")
            );

            #[cfg(target_os = "linux")]
            const SCRIPT: &str = concat!(
                include_str!("../scripts/inject-linux.js"),
                "\n",
                include_str!("../scripts/inject-common.js")
            );

            #[cfg(not(any(target_os = "windows", target_os = "linux")))]
            const SCRIPT: &str = include_str!("../scripts/inject-common.js");

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
            tauri::async_runtime::spawn(async move {
                // TODO: Replace with your actual Discord Application ID
                let client_id = "1504862803335315609"; 
                if let Ok(mut client) = DiscordIpcClient::new(client_id) {
                    if client.connect().is_ok() {
                        let _ = client.set_activity(activity::Activity::new()
                            .details("Смотрит аниме")
                            .state("На главной")
                            .assets(activity::Assets::new()
                                .large_image("logo")
                                .large_text("Anivox"))
                        );
                        let state = handle.state::<DiscordState>();
                        *state.inner().0.lock().unwrap() = Some(client);
                    }
                }
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
        .on_window_event(|window, event| match event {
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
        })
        .invoke_handler(tauri::generate_handler![
            set_discord_rpc,
            get_vpn_status,
            toggle_vpn,
            browser_action,
            report_player_status,
            content_fullscreen,
            open_in_mpv,
            check_update,
            install_update,
            get_app_version,
            shikimori_action,
            close_shikimori
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}