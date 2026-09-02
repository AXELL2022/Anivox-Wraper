use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

pub mod proxy;
use proxy::SmartProxy;

// State to hold the Discord IPC client
pub struct DiscordState(pub Mutex<Option<DiscordIpcClient>>);

// State to hold in-app VPN routing switch
pub struct VpnState(pub Arc<AtomicBool>);

const VPN_CONFIG: &str = include_str!("../../ANIVOX-UA-48.conf");

#[tauri::command]
fn get_vpn_status(state: State<'_, VpnState>) -> Result<bool, String> {
    Ok(state.0.load(Ordering::SeqCst))
}

#[tauri::command]
fn toggle_vpn(state: State<'_, VpnState>, enable: Option<bool>) -> Result<bool, String> {
    let current = state.0.load(Ordering::SeqCst);
    let target = match enable {
        Some(val) => val,
        None => !current,
    };
    state.0.store(target, Ordering::SeqCst);
    Ok(target)
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let browser_args = "--use-angle=d3d11 --enable-features=NvidiaVpSuperResolution,IntelVpSuperResolution,msEdgeVideoSuperResolution,D3D11VideoDecoder,DirectCompositionVideoOverlays --enable-nv12-dxgi-video --enable-zero-copy --ignore-gpu-blocklist --enable-gpu-rasterization --force-high-performance-gpu --enable-hardware-overlays=single-fullscreen,single-on-top,underlay --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";
    std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", browser_args);

    tauri::Builder::default()
        .manage(DiscordState(Mutex::new(None)))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let script = r#"
                (function() {
                    // Do not run inside iframes (video players, ads, third-party embeds)
                    // and never run on about:blank or invalid origins
                    if (window !== window.top || !location.href.startsWith('https://anivox.fun')) return;

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
                        vpnContainer.title = 'WireGuard VPN: Проверка статуса...';
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
                            :fullscreen #tauri-header,
                            :-webkit-full-screen #tauri-header { display: none !important; }
                            :fullscreen body,
                            :-webkit-full-screen body { margin-top: 0 !important; }
                        `;
                        document.head.appendChild(style);

                        // VPN Reactive State
                        let isVpnPending = false;
                        let isVpnOn = true;

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
                                vpnContainer.title = 'WireGuard VPN: Включен (изолирован для Anivox) — Нажмите для прямого соединения';
                            } else {
                                switchTrack.style.background = '#2a2a2a';
                                switchTrack.style.borderColor = '#444';
                                switchKnob.style.transform = 'translateX(0px)';
                                switchKnob.style.background = '#888';
                                vpnLabel.style.color = '#888';
                                vpnContainer.title = 'WireGuard VPN: Отключен (прямое соединение) — Нажмите для включения VPN';
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
                                alert('Ошибка переключения VPN: ' + err);
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

            let proxy = tauri::async_runtime::block_on(async {
                SmartProxy::start(VPN_CONFIG).await
            }).map_err(|e| format!("Failed to start SmartProxy: {}", e))?;

            let proxy_url_str = format!("socks5://127.0.0.1:{}", proxy.port);
            let vpn_state = VpnState(Arc::clone(&proxy.is_vpn_enabled));
            app.manage(vpn_state);
            app.manage(proxy);

            let _window = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External("https://anivox.fun/".parse().unwrap()),
            )
            .proxy_url(url::Url::parse(&proxy_url_str).expect("Valid proxy URL"))
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
        .invoke_handler(tauri::generate_handler![
            set_discord_rpc,
            get_vpn_status,
            toggle_vpn
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
