use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::process::Command;
use std::sync::Mutex;
use tauri::{Manager, State};

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
        use std::os::windows::process::CommandExt;
        let conf_path = ensure_config_file()?;
        let wireguard_path = if std::path::Path::new(r"C:\Program Files\WireGuard\wireguard.exe").exists() {
            r"C:\Program Files\WireGuard\wireguard.exe".to_string()
        } else {
            "wireguard.exe".to_string()
        };

        let mut cmd = Command::new(&wireguard_path);
        cmd.arg("/installtunnelservice");
        cmd.arg(&conf_path);
        cmd.creation_flags(0x08000000);

        let res = cmd.output()
            .map_err(|e| format!("Failed to start WireGuard on Windows: {}", e))?;

        if !res.status.success() {
            let err = String::from_utf8_lossy(&res.stderr);
            return Err(format!("wireguard.exe failed: {}", err.trim()));
        }

        // Wait up to 3 seconds for the service to become RUNNING
        for _ in 0..15 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            if is_vpn_active() {
                break;
            }
        }

        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err("Unsupported OS for WireGuard VPN".to_string())
    }
}

fn disconnect_vpn() -> Result<(), String> {
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
        use std::os::windows::process::CommandExt;
        let wireguard_path = if std::path::Path::new(r"C:\Program Files\WireGuard\wireguard.exe").exists() {
            r"C:\Program Files\WireGuard\wireguard.exe".to_string()
        } else {
            "wireguard.exe".to_string()
        };

        let mut cmd = Command::new(&wireguard_path);
        cmd.arg("/uninstalltunnelservice");
        cmd.arg(VPN_CONN_NAME);
        cmd.creation_flags(0x08000000);

        let res = cmd.output()
            .map_err(|e| format!("Failed to stop WireGuard on Windows: {}", e))?;

        if !res.status.success() {
            let err = String::from_utf8_lossy(&res.stderr);
            return Err(format!("wireguard.exe down failed: {}", err.trim()));
        }

        // Wait for service to stop
        for _ in 0..15 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            if !is_vpn_active() {
                break;
            }
        }

        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err("Unsupported OS for WireGuard VPN".to_string())
    }
}

#[tauri::command]
fn get_vpn_status() -> Result<bool, String> {
    Ok(is_vpn_active())
}

#[tauri::command]
fn toggle_vpn(enable: Option<bool>) -> Result<bool, String> {
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
        disconnect_vpn()?;
    }

    Ok(is_vpn_active())
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
        .manage(DiscordState(Mutex::new(None)))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // All traffic goes completely through system network settings (system DNS, routes, etc.)
            // Optional host mapping if ever needed: --host-rules="MAP anivox.fun 45.95.96.255"
            let browser_args = "--use-angle=d3d11 --enable-features=NvidiaVpSuperResolution,NvidiaVpTrueHDR,DirectCompositionVideoOverlays,DirectCompositionLetterboxVideoOptimization,DirectCompositionScalableVideoProcessing,DirectCompositionUseNV12DecodeSwapChain,UseD3D11VideoProcessor,D3D11VideoDecoder,HardwareAcceleratedVideoDecode,AllowDcompOverlaysInBackbuffer --disable-features=msEdgeVideoSuperResolution,msWebOOUI,msPdfOOUI,msSmartScreenProtection --enable-nv12-dxgi-video --ignore-gpu-blocklist --enable-gpu-rasterization --force-high-performance-gpu";
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

                    // Only inject header, UI and hotkeys into the top-level window
                    if (window !== window.top || !location.href.startsWith('https://anivox.fun')) return;

                    function setBtnStatus(text, color, resetAfterMs) {
                        const btn = document.getElementById('tauri-mpv-btn');
                        if (!btn) return;
                        btn.innerHTML = `<span style="font-size: 11px; font-weight: 700; letter-spacing: 0.5px; color: ${color};">${text}</span>`;
                        if (resetAfterMs) {
                            setTimeout(() => {
                                btn.innerHTML = '<span style="font-size: 11px; font-weight: 700; letter-spacing: 0.5px; color: #ffa9de;">▶ MPV</span>';
                            }, resetAfterMs);
                        }
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

                    function setupHeader() {
                        if (document.getElementById('tauri-header')) return;

                        const header = document.createElement('div');
                        header.id = 'tauri-header';
                        header.style.cssText = `
                            position: fixed;
                            top: 0;
                            left: 0;
                            width: 100%;
                            height: 32px;
                            background: #111;
                            color: white;
                            display: flex;
                            align-items: center;
                            padding: 0 10px;
                            z-index: 999999;
                            border-bottom: 1px solid #333;
                            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
                            user-select: none;
                            -webkit-user-select: none;
                            box-sizing: border-box;
                        `;

                        const btnStyle = `
                            background: transparent;
                            border: none;
                            color: #ccc;
                            width: 28px;
                            height: 28px;
                            display: flex;
                            align-items: center;
                            justify-content: center;
                            cursor: pointer;
                            font-size: 16px;
                            border-radius: 4px;
                            transition: background 0.15s, color 0.15s;
                            margin-right: 4px;
                        `;

                        function createButton(html, onClick, title) {
                            const btn = document.createElement('button');
                            btn.innerHTML = html;
                            btn.style.cssText = btnStyle;
                            btn.title = title;
                            btn.onclick = onClick;
                            btn.onmouseenter = () => { btn.style.background = '#333'; btn.style.color = 'white'; };
                            btn.onmouseleave = () => { btn.style.background = 'transparent'; btn.style.color = '#ccc'; };
                            return btn;
                        }

                        const backBtn = createButton('&#10094;', () => window.history.back(), 'Назад');
                        const forwardBtn = createButton('&#10095;', () => window.history.forward(), 'Вперед');
                        const reloadBtn = createButton('&#8635;', () => window.location.reload(), 'Обновить');

                        header.appendChild(backBtn);
                        header.appendChild(forwardBtn);
                        header.appendChild(reloadBtn);

                        // Vertical divider
                        const divider = document.createElement('div');
                        divider.style.cssText = 'width: 1px; height: 16px; background: #333; margin: 0 6px;';
                        header.appendChild(divider);

                        // VPN Toggle component
                        const vpnContainer = document.createElement('div');
                        vpnContainer.id = 'tauri-vpn-container';
                        vpnContainer.style.cssText = `
                            display: flex;
                            align-items: center;
                            gap: 6px;
                            cursor: pointer;
                            padding: 2px 7px;
                            border-radius: 6px;
                            transition: background 0.15s;
                            user-select: none;
                            -webkit-user-select: none;
                        `;
                        vpnContainer.title = 'WireGuard VPN: Отключен — Нажмите для подключения';
                        vpnContainer.onmouseenter = () => { vpnContainer.style.background = '#222'; };
                        vpnContainer.onmouseleave = () => { vpnContainer.style.background = 'transparent'; };

                        const vpnLabel = document.createElement('span');
                        vpnLabel.innerText = 'VPN';
                        vpnLabel.style.cssText = 'font-size: 11px; font-weight: 700; letter-spacing: 0.5px; color: #888; transition: color 0.2s;';

                        const switchTrack = document.createElement('div');
                        switchTrack.style.cssText = 'width: 28px; height: 16px; background: #2a2a2a; border-radius: 999px; position: relative; transition: background 0.2s, border-color 0.2s; border: 1px solid #444; box-sizing: border-box;';

                        const switchKnob = document.createElement('div');
                        switchKnob.style.cssText = 'width: 10px; height: 10px; background: #888; border-radius: 50%; position: absolute; top: 2px; left: 2px; transition: transform 0.2s ease, background 0.2s;';
                        switchTrack.appendChild(switchKnob);

                        vpnContainer.appendChild(vpnLabel);
                        vpnContainer.appendChild(switchTrack);
                        header.appendChild(vpnContainer);

                        // Vertical divider
                        const divider2 = document.createElement('div');
                        divider2.style.cssText = 'width: 1px; height: 16px; background: #333; margin: 0 6px;';
                        header.appendChild(divider2);

                        // MPV launch button
                        const mpvBtn = document.createElement('button');
                        mpvBtn.id = 'tauri-mpv-btn';
                        mpvBtn.innerHTML = '<span style="font-size: 11px; font-weight: 700; letter-spacing: 0.5px;">▶ MPV</span>';
                        mpvBtn.title = 'Воспроизвести текущее видео в MPV (Горячая клавиша: M)';
                        mpvBtn.style.cssText = `
                            background: #1f1f23;
                            border: 1px solid #38383f;
                            color: #ffa9de;
                            padding: 2px 8px;
                            height: 22px;
                            display: flex;
                            align-items: center;
                            justify-content: center;
                            cursor: pointer;
                            border-radius: 4px;
                            transition: background 0.15s, color 0.15s, border-color 0.15s;
                            user-select: none;
                            -webkit-user-select: none;
                        `;
                        mpvBtn.onmouseenter = () => { mpvBtn.style.background = '#2d2d35'; mpvBtn.style.borderColor = '#ffa9de'; };
                        mpvBtn.onmouseleave = () => { mpvBtn.style.background = '#1f1f23'; mpvBtn.style.borderColor = '#38383f'; };
                        mpvBtn.onclick = launchInMpv;
                        header.appendChild(mpvBtn);

                        // Center title
                        const title = document.createElement('div');
                        title.innerText = 'Anivox';
                        title.style.cssText = 'position: absolute; left: 50%; transform: translateX(-50%); font-size: 12px; color: #888; letter-spacing: 1.5px; text-transform: uppercase; font-weight: bold; pointer-events: none;';
                        header.appendChild(title);

                        document.body.appendChild(header);

                        // Push content down
                        const style = document.createElement('style');
                        style.textContent = `
                            body { margin-top: 32px !important; }
                            #tauri-header button:active { background: #444 !important; }
                            #tauri-vpn-container:active { transform: scale(0.97); }
                            #tauri-mpv-btn:active { transform: scale(0.96) !important; background: #383842 !important; }
                            :fullscreen #tauri-header,
                            :-webkit-full-screen #tauri-header { display: none !important; }
                            :fullscreen body,
                            :-webkit-full-screen body { margin-top: 0 !important; }

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

                        // VPN Reactive State
                        let isVpnPending = false;
                        let isVpnOn = false;

                        function renderVpnState(active, pending) {
                            isVpnOn = !!active;
                            isVpnPending = !!pending;

                            if (isVpnPending) {
                                vpnContainer.style.opacity = '0.5';
                                vpnContainer.style.pointerEvents = 'none';
                                vpnContainer.title = 'Переключение WireGuard VPN...';
                                return;
                            }

                            vpnContainer.style.opacity = '1';
                            vpnContainer.style.pointerEvents = 'auto';

                            if (isVpnOn) {
                                switchTrack.style.background = '#10b981';
                                switchTrack.style.borderColor = '#059669';
                                switchKnob.style.transform = 'translateX(12px)';
                                switchKnob.style.background = '#ffffff';
                                vpnLabel.style.color = '#10b981';
                                vpnContainer.title = 'WireGuard VPN: Подключен (ANIVOX-UA-48) — Нажмите для отключения';
                            } else {
                                switchTrack.style.background = '#2a2a2a';
                                switchTrack.style.borderColor = '#444';
                                switchKnob.style.transform = 'translateX(0px)';
                                switchKnob.style.background = '#888';
                                vpnLabel.style.color = '#888';
                                vpnContainer.title = 'WireGuard VPN: Отключен — Нажмите для подключения';
                            }
                        }

                        async function syncVpnStatus() {
                            if (isVpnPending) return;
                            try {
                                if (window.__TAURI__ && window.__TAURI__.core) {
                                    const status = await window.__TAURI__.core.invoke('get_vpn_status');
                                    renderVpnState(status, false);
                                }
                            } catch (e) {
                                console.error('Failed to get VPN status:', e);
                            }
                        }

                        vpnContainer.onclick = async () => {
                            if (isVpnPending) return;
                            const nextState = !isVpnOn;
                            renderVpnState(nextState, true);
                            try {
                                const res = await window.__TAURI__.core.invoke('toggle_vpn', { enable: nextState });
                                renderVpnState(res, false);
                                location.reload();
                            } catch (err) {
                                console.error('Failed to toggle VPN:', err);
                                alert('Ошибка переключения WireGuard: ' + err);
                                await syncVpnStatus();
                            }
                        };

                        syncVpnStatus();
                        setInterval(syncVpnStatus, 10000);
                    }

                    if (document.readyState === 'loading') {
                        document.addEventListener('DOMContentLoaded', setupHeader);
                    } else {
                        setupHeader();
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

            // Ensure WireGuard VPN is OFF by default on app launch
            if is_vpn_active() {
                let _ = disconnect_vpn();
            }

            std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", browser_args);

            let _window = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External("https://anivox.fun/".parse().unwrap()),
            )
            .title("Anivox")
            .inner_size(1280.0, 720.0)
            .initialization_script(script)
            // Optimized GPU, Video and RTX Video Super Resolution (VSR) settings
            .additional_browser_args(browser_args)
            .build()?;

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
            tauri::WindowEvent::Destroyed => {
                if is_vpn_active() {
                    let _ = disconnect_vpn();
                }
                window.app_handle().exit(0);
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            set_discord_rpc,
            get_vpn_status,
            toggle_vpn,
            open_in_mpv
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
