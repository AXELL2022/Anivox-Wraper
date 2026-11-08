//! macOS shell integration: the native menu bar (required for Cmd+C/V/X/A in
//! the webviews and for Cmd+Q) and showing the window again from the Dock.

use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};

pub(crate) fn install_menu(app: &tauri::App) -> tauri::Result<()> {
    let app_menu = SubmenuBuilder::new(app, "Anivox")
        .about(None)
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .item(
            &MenuItemBuilder::with_id("mac-quit", "Выйти из Anivox")
                .accelerator("Cmd+Q")
                .build(app)?,
        )
        .build()?;
    let edit = SubmenuBuilder::new(app, "Правка")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let navigation = SubmenuBuilder::new(app, "Навигация")
        .item(
            &MenuItemBuilder::with_id("mac-back", "Назад")
                .accelerator("Cmd+[")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("mac-forward", "Вперёд")
                .accelerator("Cmd+]")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("mac-reload", "Обновить")
                .accelerator("Cmd+R")
                .build(app)?,
        )
        .build()?;
    let window = SubmenuBuilder::new(app, "Окно")
        .minimize()
        .maximize()
        .close_window()
        .build()?;
    let menu = MenuBuilder::new(app)
        .items(&[&app_menu, &edit, &navigation, &window])
        .build()?;
    app.set_menu(menu)?;
    Ok(())
}

pub(crate) fn on_menu_event(app: &tauri::AppHandle, id: &str) {
    let result = match id {
        "mac-quit" => {
            crate::quit_app(app);
            return;
        }
        "mac-back" => crate::run_browser_action(app, "back"),
        "mac-forward" => crate::run_browser_action(app, "forward"),
        "mac-reload" => crate::run_browser_action(app, "reload"),
        _ => return,
    };
    if let Err(err) = result {
        eprintln!("Не удалось выполнить действие меню: {err}");
    }
}

/// Clicking the Dock icon while the window is hidden in the tray shows it again.
pub(crate) fn on_run_event(app: &tauri::AppHandle, event: &tauri::RunEvent) {
    if let tauri::RunEvent::Reopen {
        has_visible_windows: false,
        ..
    } = event
    {
        crate::show_primary_window(app);
    }
}

/// Temporary diagnostics for the toolbar layout on macOS: appends the window
/// size and the real webview bounds to `<app_local_data_dir>/logs/layout.log`
/// (first lines only), so a wrong offset can be read off instead of guessed.
pub(crate) fn log_layout(window: &tauri::Window) {
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tauri::Manager;

    static LINES: AtomicUsize = AtomicUsize::new(0);
    if LINES.fetch_add(1, Ordering::Relaxed) >= 40 {
        return;
    }
    let Ok(dir) = window.app_handle().path().app_local_data_dir() else {
        return;
    };
    let dir = dir.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let bounds = |label: &str| {
        window
            .get_webview(label)
            .map(|view| format!("{:?}", view.bounds()))
            .unwrap_or_else(|| "-".into())
    };
    let line = format!(
        "inner={:?} outer={:?} scale={:?} fullscreen={} toolbar={} content={}\n",
        window.inner_size(),
        window.outer_size(),
        window.scale_factor(),
        window
            .state::<crate::BrowserState>()
            .fullscreen
            .load(Ordering::Relaxed),
        bounds("toolbar"),
        bounds("content"),
    );
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("layout.log"))
    {
        let _ = file.write_all(line.as_bytes());
    }
}
