use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::process::Command;
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};

#[cfg(not(target_os = "linux"))]
const TOOLBAR_HEIGHT: f64 = 40.0;

#[cfg(target_os = "windows")]
const BROWSER_ARGS: &str = "--use-angle=d3d11 --enable-features=NvidiaVpSuperResolution,NvidiaVpTrueHDR,DirectCompositionVideoOverlays,DirectCompositionLetterboxVideoOptimization,DirectCompositionScalableVideoProcessing,DirectCompositionUseNV12DecodeSwapChain,UseD3D11VideoProcessor,D3D11VideoDecoder,HardwareAcceleratedVideoDecode,AllowDcompOverlaysInBackbuffer --disable-features=msEdgeVideoSuperResolution,msWebOOUI,msPdfOOUI,msSmartScreenProtection --enable-nv12-dxgi-video --ignore-gpu-blocklist --enable-gpu-rasterization --force-high-performance-gpu";

#[cfg(not(target_os = "linux"))]
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
    Ok(())
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(BrowserState {
            url: Mutex::new("https://anivox.fun/".parse().unwrap()),
            fullscreen: std::sync::atomic::AtomicBool::new(false),
        })
        .manage(DiscordState(Mutex::new(None)))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // All traffic goes completely through system network settings (system DNS, routes, etc.)
            // Optional host mapping if ever needed: --host-rules="MAP anivox.fun 45.95.96.255"
            let script = r#"
                (function() {
                    // Multi-layer video stream & subtitle interceptor for MPV
                    let apiLinks = null;
                    let currentHlsUrl = null;
                    let lastStreamUrl = null;
                    let lastSubUrl = null;

                    function resolveUrl(u) {
                        if (!u || typeof u !== 'string') return null;
                        if (u.startsWith('//')) return 'https:' + u;
                        if (u.startsWith('http://')) return u.replace('http://', 'https://');
                        if (!u.startsWith('http')) return 'https://' + u;
                        return u;
                    }

                    // 1. Hook XMLHttpRequest for episodes API and stream manifests
                    try {
                        const origXhrOpen = XMLHttpRequest.prototype.open;
                        const origXhrSend = XMLHttpRequest.prototype.send;
                        XMLHttpRequest.prototype.open = function(method, url) {
                            this._anivoxUrl = typeof url === 'string' ? url : (url && url.href ? url.href : String(url));
                            return origXhrOpen.apply(this, arguments);
                        };
                        XMLHttpRequest.prototype.send = function() {
                            this.addEventListener('load', function() {
                                try {
                                    const u = this._anivoxUrl || '';
                                    if (u.includes('episodes/')) {
                                        const data = JSON.parse(this.responseText);
                                        if (data && data.links) {
                                            apiLinks = data.links;
                                            console.log('[Anivox-MPV] Intercepted episode links from XHR:', apiLinks);
                                        }
                                    }
                                    if (u.includes('/api/hls/')) {
                                        currentHlsUrl = u;
                                        lastStreamUrl = u;
                                        console.log('[Anivox-MPV] Intercepted active HLS URL from XHR:', u);
                                    } else if (u.includes('.m3u8') || u.includes('.mp4') || u.includes('/stream')) {
                                        if (!u.includes('.ts') && !u.includes('segment') && !u.includes('/audio/') && !u.includes('/subs/')) {
                                            lastStreamUrl = u;
                                            console.log('[Anivox-MPV] Intercepted stream URL from XHR:', u);
                                        }
                                    }
                                    if (u.includes('.ass')) {
                                        lastSubUrl = u;
                                    }
                                } catch (e) {}
                            });
                            return origXhrSend.apply(this, arguments);
                        };

                        // 2. Hook Fetch API
                        const origFetch = window.fetch;
                        window.fetch = async function(input, init) {
                            const u = typeof input === 'string' ? input : (input && input.url ? input.url : '');
                            const response = await origFetch.apply(this, arguments);
                            try {
                                if (u.includes('episodes/')) {
                                    const clone = response.clone();
                                    clone.json().then(data => {
                                        if (data && data.links) {
                                            apiLinks = data.links;
                                            console.log('[Anivox-MPV] Intercepted episode links from fetch:', apiLinks);
                                        }
                                    }).catch(() => {});
                                }
                                if (u.includes('/api/hls/')) {
                                    currentHlsUrl = u;
                                    lastStreamUrl = u;
                                    console.log('[Anivox-MPV] Intercepted active HLS URL from fetch:', u);
                                } else if (u.includes('.m3u8') || u.includes('.mp4') || u.includes('/stream')) {
                                    if (!u.includes('.ts') && !u.includes('segment') && !u.includes('/audio/') && !u.includes('/subs/')) {
                                        lastStreamUrl = u;
                                        console.log('[Anivox-MPV] Intercepted stream URL from fetch:', u);
                                    }
                                }
                                if (u.includes('.ass')) {
                                    lastSubUrl = u;
                                }
                            } catch (e) {}
                            return response;
                        };
                    } catch (e) {
                        console.error('[Anivox-MPV] Failed to hook network:', e);
                    }

                    // The local toolbar is separate; only player hooks belong in the site.
                    if (window !== window.top || location.origin !== 'https://anivox.fun') return;

                    function setBtnStatus(text, color, resetAfterMs) {
                        window.__TAURI__.core.invoke('report_player_status', { text, color, resetAfterMs }).catch(console.error);
                    }

                    function parseQualityHeight(q) {
                        if (!q) return 0;
                        const s = String(q).toUpperCase().replace('~', '').replace('_UPSCALE', '').trim();
                        if (s === '4K' || s.includes('2160')) return 2160;
                        if (s === '2K' || s.includes('1440')) return 1440;
                        return parseInt(s) || 0;
                    }

                    function selectMaxStreamUrl(links) {
                        if (!links) return null;
                        if (typeof links === 'string') return { url: resolveUrl(links), quality: 'MAX' };

                        // Sort descending by resolution (4K / 2160 > 2K / 1440 > 1080 > 720 > 480 > 360)
                        const sortedKeys = Object.keys(links).sort((a, b) => parseQualityHeight(b) - parseQualityHeight(a));
                        for (const k of sortedKeys) {
                            if (links[k] && links[k] !== 'premium' && typeof links[k] === 'string') {
                                console.log('[Anivox-MPV] Forced MAX quality selected:', k, '->', links[k]);
                                return { url: resolveUrl(links[k]), quality: k };
                            }
                        }

                        const firstK = Object.keys(links)[0];
                        return { url: resolveUrl(links[firstK]), quality: firstK };
                    }

                    function findActiveStream() {
                        // 1. Captured API links from episodes endpoint - FORCED MAX QUALITY!
                        if (apiLinks && typeof apiLinks === 'object') {
                            const res = selectMaxStreamUrl(apiLinks);
                            if (res && res.url) {
                                console.log('[Anivox-MPV] Selected MAX quality stream from apiLinks:', res.quality, ':', res.url);
                                return { url: res.url, time: 0, quality: res.quality };
                            }
                        }

                        // 2. Direct inspection of Vue 3 component state (.player-container)
                        const playerEls = [
                            document.querySelector('.player-container'),
                            document.querySelector('video.video-element'),
                            document.querySelector('video')
                        ];
                        for (const el of playerEls) {
                            if (!el) continue;
                            const comp = el.__vueParentComponent || el._vnode?.component || el.__vue_app__;
                            if (comp) {
                                const ctx = comp.ctx || comp.proxy || comp.setupState;
                                if (ctx) {
                                    if (ctx.links && typeof ctx.links === 'object') {
                                        const res = selectMaxStreamUrl(ctx.links);
                                        if (res && res.url) {
                                            console.log('[Anivox-MPV] Selected MAX quality stream from Vue ctx.links:', res.quality, ':', res.url);
                                            return { url: res.url, time: ctx.currentTime || 0, quality: res.quality };
                                        }
                                    }
                                    if (ctx.videoSrc && typeof ctx.videoSrc === 'string' && !ctx.videoSrc.startsWith('blob:')) {
                                        console.log('[Anivox-MPV] Found stream in Vue ctx.videoSrc:', ctx.videoSrc);
                                        return { url: resolveUrl(ctx.videoSrc), time: ctx.currentTime || 0, quality: 'MAX' };
                                    }
                                    if (ctx.hls && ctx.hls.url) {
                                        console.log('[Anivox-MPV] Found stream in Vue ctx.hls.url:', ctx.hls.url);
                                        return { url: resolveUrl(ctx.hls.url), time: ctx.currentTime || 0, quality: 'MAX' };
                                    }
                                }
                            }
                        }

                        // 3. Active HLS stream from network
                        if (currentHlsUrl) {
                            console.log('[Anivox-MPV] Fallback to currentHlsUrl:', currentHlsUrl);
                            return { url: resolveUrl(currentHlsUrl), time: 0, quality: 'MAX' };
                        }

                        // 4. Other network intercepted stream URL (XHR / Fetch)
                        if (lastStreamUrl && typeof lastStreamUrl === 'string' && !lastStreamUrl.startsWith('blob:')) {
                            console.log('[Anivox-MPV] Fallback to lastStreamUrl:', lastStreamUrl);
                            return { url: resolveUrl(lastStreamUrl), time: 0, quality: 'MAX' };
                        }

                        // 5. HTML5 video tag currentSrc / src
                        const video = document.querySelector('video');
                        if (video) {
                            if (video.currentSrc && !video.currentSrc.startsWith('blob:')) {
                                return { url: resolveUrl(video.currentSrc), time: video.currentTime || 0, quality: 'MAX' };
                            }
                            if (video.src && !video.src.startsWith('blob:')) {
                                return { url: resolveUrl(video.src), time: video.currentTime || 0, quality: 'MAX' };
                            }
                        }

                        // 6. Fallback iframe player (Kodik / Sibnet etc.)
                        const iframe = document.querySelector('iframe');
                        if (iframe && iframe.src && iframe.src.startsWith('http')) {
                            console.log('[Anivox-MPV] Found stream in fallback iframe:', iframe.src);
                            return { url: iframe.src, time: 0, quality: 'iframe' };
                        }

                        return null;
                    }

                    async function launchInMpv() {
                        setBtnStatus('⌛ Поиск...', '#ffeb3b', 0);
                        const video = document.querySelector('video.video-element') || document.querySelector('video');
                        const info = findActiveStream();

                        if (!info || !info.url) {
                            console.warn('[Anivox-MPV] Stream URL not found yet');
                            setBtnStatus('⚠ Включите серию!', '#ff5252', 2500);
                            return;
                        }

                        const startTime = (video && video.currentTime) ? video.currentTime : (info.time || 0);
                        if (video && !video.paused) {
                            video.pause();
                        }

                        const qLabel = info.quality && info.quality !== 'auto' ? ` (${info.quality})` : ' (MAX)';
                        setBtnStatus(`▶ Запуск MPV${qLabel}...`, '#69f0ae', 0);

                        let title = document.title || 'Anivox';
                        title = title.replace(/ — Anivox| - Anivox| \| Anivox/gi, '').trim();
                        const token = localStorage.getItem('token') || null;

                        try {
                            if (window.__TAURI__ && window.__TAURI__.core) {
                                await window.__TAURI__.core.invoke('open_in_mpv', {
                                    url: info.url,
                                    title: title || null,
                                    startTime: startTime > 0 ? startTime : null,
                                    subUrl: lastSubUrl || null,
                                    token: token
                                });
                                setBtnStatus(`✔ Запущен${qLabel}!`, '#69f0ae', 2500);
                            } else {
                                throw new Error('Tauri API недоступен');
                            }
                        } catch (err) {
                            console.error('[Anivox-MPV] Launch error:', err);
                            setBtnStatus('⚠ Ошибка!', '#ff5252', 3000);
                        }
                    }

                    window.__anivoxLaunchMpv = launchInMpv;
                    document.addEventListener('fullscreenchange', () => {
                        window.__TAURI__.core.invoke('content_fullscreen', {
                            fullscreen: !!document.fullscreenElement
                        }).catch(console.error);
                    });

                    window.addEventListener('keydown', (e) => {
                        const tag = document.activeElement ? document.activeElement.tagName : '';
                        if (tag === 'INPUT' || tag === 'TEXTAREA' || (document.activeElement && document.activeElement.isContentEditable)) {
                            return;
                        }
                        if (e.key === 'm' || e.key === 'M' || e.key === 'ь' || e.key === 'Ь') {
                            e.preventDefault();
                            launchInMpv();
                        }
                    });

                    function setupVideoStyles() {
                        const style = document.createElement('style');
                        style.textContent = `
                            /* DirectComposition Video Overlay & NVIDIA RTX VSR Fixes */
                            .player-container, .player, [class*="player"], .video-element, video, iframe {
                                border-radius: 0 !important;
                                -webkit-mask: none !important;
                                mask: none !important;
                                filter: none !important;
                                backdrop-filter: none !important;
                                transform: none !important;
                            }
                            video.video-element, video {
                                background-color: transparent !important;
                            }
                            .touch-zone {
                                background: transparent !important;
                            }
                            .libassjs-canvas {
                                pointer-events: none !important;
                            }
                        `;
                        document.head.appendChild(style);

                    }

                    if (document.readyState === 'loading') {
                        document.addEventListener('DOMContentLoaded', setupVideoStyles);
                    } else {
                        setupVideoStyles();
                    }

                    // Discord RPC logic with deduplication and debouncing
                    let lastRpcTitle = '';
                    let rpcDebounceTimer = null;

                    function updateDiscord() {
                        const t = document.title || '';
                        let d = t.endsWith(' - Anivox') ? t.replace(' - Anivox', '') : t;
                        if (!d || d === lastRpcTitle) return;
                        lastRpcTitle = d;

                        if (rpcDebounceTimer) clearTimeout(rpcDebounceTimer);
                        rpcDebounceTimer = setTimeout(() => {
                            if (window.__TAURI__ && window.__TAURI__.core) {
                                window.__TAURI__.core.invoke('set_discord_rpc', { details: 'Смотрит: ' + d, stateText: 'Anivox' }).catch(() => {});
                            }
                        }, 1000);
                    }
                    const o = new MutationObserver(updateDiscord);
                    const t = document.querySelector('title');
                    if (t) o.observe(t, { subtree: true, characterData: true, childList: true });
                    setInterval(updateDiscord, 30000);
                    updateDiscord();
                })();
            "#;

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

            #[cfg(target_os = "linux")]
            if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
                std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
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
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } => {
                let window = window.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let _ = layout_webviews(&window);
                });
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
            open_in_mpv
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}