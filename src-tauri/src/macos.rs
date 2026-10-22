//! Minimal macOS application. All application actions originate in native menus;
//! the remote webview has no application commands or integration scripts.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_updater::UpdaterExt;

// Deliberately does not match any integration-capable webview in capabilities/.
const WINDOW_LABEL: &str = "macos-main";

#[derive(Default)]
struct UpdateState(Arc<AtomicBool>);

// Keep the complete check / confirmation / installation transaction exclusive,
// including while the user is reading the native confirmation dialog.
struct UpdateOperation(Arc<AtomicBool>);

impl Drop for UpdateOperation {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn notify(app: &AppHandle, message: impl Into<String>) {
    app.dialog().message(message).title("Anivox").show(|_| {});
}

async fn confirm_install(app: &AppHandle, message: String) -> Result<bool, String> {
    let app = app.clone();
    // The dialog plugin explicitly forbids blocking_show on the main thread.
    // A blocking worker can wait for the native UI without blocking its event loop.
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .message(message)
            .title("Anivox Update")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Install and Restart".into(),
                "Cancel".into(),
            ))
            .blocking_show()
    })
    .await
    .map_err(|error| error.to_string())
}

async fn check_and_install(app: &AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|error| error.to_string())?;
    let Some(reviewed) = updater.check().await.map_err(|error| error.to_string())? else {
        notify(app, "Anivox is up to date.");
        return Ok(());
    };

    let notes = reviewed
        .body
        .as_deref()
        .unwrap_or("No release notes provided.");
    let prompt = format!(
        "Anivox {} is available (installed: {}).\n\n{}\n\nInstall this update now? Anivox will restart after installation.",
        reviewed.version, reviewed.current_version, notes
    );
    if !confirm_install(app, prompt).await? {
        return Ok(());
    }

    // Do not install a new target that appeared while the user reviewed another
    // release. Also pin the signed artifact, not merely its version string.
    let Some(update) = updater.check().await.map_err(|error| error.to_string())? else {
        notify(
            app,
            "The reviewed update is no longer available. Check for updates again.",
        );
        return Ok(());
    };
    if update.version != reviewed.version
        || update.download_url != reviewed.download_url
        || update.signature != reviewed.signature
    {
        notify(
            app,
            "The available update changed after confirmation. Nothing was installed. Check for updates again to review the new release.",
        );
        return Ok(());
    }

    // download() verifies the configured updater signature before install().
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || update.install(bytes))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    app.restart();
}

fn check_updates(app: &AppHandle) {
    let busy = app.state::<UpdateState>().0.clone();
    if busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let operation = UpdateOperation(busy);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _operation = operation;
        if let Err(error) = check_and_install(&app).await {
            notify(&app, format!("Could not update Anivox:\n\n{error}"));
        }
    });
}

pub fn run() {
    tauri::Builder::default()
        .manage(UpdateState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // No invoke_handler: the remote site cannot call any application command.
        .setup(|app| {
            let app_menu = SubmenuBuilder::new(app, "Anivox")
                .about(None)
                .text("check-updates", "Check for Updates…")
                .separator()
                .hide()
                .hide_others()
                .show_all()
                .separator()
                .quit()
                .build()?;
            let back = MenuItemBuilder::with_id("back", "Back")
                .accelerator("Cmd+[")
                .build(app)?;
            let forward = MenuItemBuilder::with_id("forward", "Forward")
                .accelerator("Cmd+]")
                .build(app)?;
            let reload = MenuItemBuilder::with_id("reload", "Reload")
                .accelerator("Cmd+R")
                .build(app)?;
            let navigation = SubmenuBuilder::new(app, "Navigation")
                .item(&back)
                .item(&forward)
                .item(&reload)
                .build()?;
            let edit = SubmenuBuilder::new(app, "Edit")
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?;
            let window = SubmenuBuilder::new(app, "Window")
                .minimize()
                .maximize()
                .fullscreen()
                .close_window()
                .build()?;
            let menu = MenuBuilder::new(app)
                .items(&[&app_menu, &edit, &navigation, &window])
                .build()?;
            app.set_menu(menu)?;

            WebviewWindowBuilder::new(
                app,
                WINDOW_LABEL,
                WebviewUrl::External("https://anivox.fun/".parse()?),
            )
            .title("Anivox")
            .inner_size(1280.0, 800.0)
            .min_inner_size(640.0, 480.0)
            .resizable(true)
            .maximizable(true)
            .build()?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "check-updates" {
                check_updates(app);
                return;
            }
            if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
                let result = match event.id().as_ref() {
                    "back" => window.eval("window.history.back()"),
                    "forward" => window.eval("window.history.forward()"),
                    "reload" => window.reload(),
                    _ => return,
                };
                if let Err(error) = result {
                    notify(app, format!("Navigation failed:\n\n{error}"));
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run the minimal macOS application");
}
