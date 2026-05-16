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
            let script = "(function(){function u(){const t=document.title;let d=t.endsWith(' - Anivox')?t.replace(' - Anivox',''):t;window.__TAURI__.core.invoke('set_discord_rpc',{details:'Смотрит: '+d,stateText:'Anivox'}).catch(console.error)}const o=new MutationObserver(u);const t=document.querySelector('title');if(t)o.observe(t,{subtree:true,characterData:true,childList:true});setInterval(u,30000);u()})()";

            let _window = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External("https://anivox.fun/".parse().unwrap()),
            )
            .title("Anivox")
            .inner_size(1280.0, 720.0)
            .initialization_script(script)
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
