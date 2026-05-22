use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::sync::Mutex;
use tauri::{Manager, State};

// State to hold the Discord IPC client
pub struct DiscordState(pub Mutex<Option<DiscordIpcClient>>);

#[tauri::command]
fn set_discord_rpc(
    state: State<'_, DiscordState>,
    details: Option<String>,
    state_text: Option<String>,
) -> Result<(), String> {
    let mut client_guard = state.0.lock().unwrap();

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
    tauri::Builder::default()
        .manage(DiscordState(Mutex::new(None)))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let script = r#"
                (function() {
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
                            font-family: sans-serif;
                            user-select: none;
                            -webkit-user-select: none;
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
                            font-size: 18px;
                            border-radius: 4px;
                            transition: background 0.2s, color 0.2s;
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

                        const title = document.createElement('div');
                        title.innerText = 'Anivox';
                        title.style.cssText = 'flex-grow: 1; text-align: center; font-size: 12px; color: #888; letter-spacing: 1px; text-transform: uppercase; font-weight: bold; margin-right: 96px;';
                        header.appendChild(title);

                        document.body.appendChild(header);
                        
                        // Push content down
                        const style = document.createElement('style');
                        style.textContent = `
                            body { margin-top: 32px !important; }
                            #tauri-header button:active { background: #444 !important; }
                        `;
                        document.head.appendChild(style);
                    }

                    if (document.readyState === 'loading') {
                        document.addEventListener('DOMContentLoaded', setupHeader);
                    } else {
                        setupHeader();
                    }

                    // Discord RPC logic
                    function updateDiscord() {
                        const t = document.title;
                        let d = t.endsWith(' - Anivox') ? t.replace(' - Anivox', '') : t;
                        window.__TAURI__.core.invoke('set_discord_rpc', { details: 'Смотрит: ' + d, stateText: 'Anivox' }).catch(console.error);
                    }
                    const o = new MutationObserver(updateDiscord);
                    const t = document.querySelector('title');
                    if (t) o.observe(t, { subtree: true, characterData: true, childList: true });
                    setInterval(updateDiscord, 30000);
                    updateDiscord();
                })();
            "#;

            let _window = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External("https://anivox.fun/".parse().unwrap()),
            )
            .title("Anivox")
            .inner_size(1280.0, 720.0)
            .initialization_script(script)
            // Optimized GPU and Video settings
            .additional_browser_args("--use-angle=d3d11 --enable-features=VaapiVideoDecoder,D3D11VideoDecoder --enable-zero-copy --ignore-gpu-blocklist --disable-background-timer-throttling --disable-backgrounding-occluded-windows --disable-renderer-backgrounding --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,CalculateNativeWinOcclusion,IntensiveWakeUpThrottling")
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
                        *state.0.lock().unwrap() = Some(client);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![set_discord_rpc])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
