//! Minimal native Linux shell for Anivox: GTK4 window, header bar with
//! navigation buttons and a WebKitGTK 6.0 view. No Tauri involved; the
//! platform-independent logic comes from `anivox-core`.

mod connection;
mod ipc;
mod session;
mod settings;
mod shikimori;
mod site_recovery;
mod tray;
mod vblank;

use anivox_core::site_hosts;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use url::Url;
use webkit6::prelude::*;
use webkit6::{
    NavigationPolicyDecision, NavigationType, PolicyDecisionType, UserContentInjectedFrames,
    UserScript, UserScriptInjectionTime, WebView,
};

const APP_ID: &str = "com.yasin.anivox.gtk";
const IPC_HANDLER: &str = "anivox";
/// Same identifier as the Tauri build, so both shells share settings and history.
const APP_DIR: &str = "com.yasin.anivox";

// The same site scripts the Tauri shell injects on Linux, in the same order.
const SITE_SCRIPTS: [&str; 7] = [
    include_str!("../../src-tauri/scripts/inject-linux.js"),
    include_str!("../../src-tauri/scripts/mpv-session.js"),
    include_str!("../../src-tauri/scripts/inject-common.js"),
    include_str!("../../src-tauri/scripts/player-fullscreen.js"),
    include_str!("../../src-tauri/scripts/common-styles.js"),
    include_str!("../../src-tauri/scripts/site-lifecycle.js"),
    include_str!("../../src-tauri/scripts/segment-monitor.js"),
];
const IPC_SHIM: &str = include_str!("../scripts/ipc-shim.js");

/// Page-provided colors go into Pango markup: accept only `#rgb`/`#rrggbb[aa]`.
fn safe_color(value: &str) -> &str {
    let hex = value.strip_prefix('#').unwrap_or("");
    if matches!(hex.len(), 3 | 6 | 8) && hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        value
    } else {
        "#cccccc"
    }
}

/// Main-thread widgets that react to requests from the IPC worker threads.
struct Shell {
    window: gtk4::ApplicationWindow,
    header: gtk4::HeaderBar,
    status: gtk4::Label,
    status_generation: Cell<u64>,
    recovery: Rc<site_recovery::Recovery>,
    // Owns the overlay state; the toolbar button holds only a weak reference.
    _shikimori: Rc<shikimori::Shikimori>,
}

impl Shell {
    fn handle(self: &Rc<Self>, event: ipc::UiEvent) {
        match event {
            ipc::UiEvent::Fullscreen(on) => {
                self.window.set_fullscreened(on);
                self.header.set_visible(!on);
            }
            ipc::UiEvent::SiteReady(generation) => self.recovery.finish(generation),
            ipc::UiEvent::Status {
                text,
                color,
                reset_after_ms,
                detail,
            } => self.show_status(&text, &color, reset_after_ms, detail.as_deref()),
        }
    }

    fn show_status(
        self: &Rc<Self>,
        text: &str,
        color: &str,
        reset_after_ms: u64,
        detail: Option<&str>,
    ) {
        let markup = format!(
            "<span foreground=\"{}\">{}</span>",
            safe_color(color),
            glib::markup_escape_text(text)
        );
        self.status.set_markup(&markup);
        self.status.set_tooltip_text(detail);
        self.status.set_visible(true);
        let mine = self.status_generation.get() + 1;
        self.status_generation.set(mine);
        if reset_after_ms > 0 {
            let shell = Rc::downgrade(self);
            glib::timeout_add_local_once(
                std::time::Duration::from_millis(reset_after_ms),
                move || {
                    if let Some(shell) = shell.upgrade() {
                        if shell.status_generation.get() == mine {
                            shell.status.set_visible(false);
                        }
                    }
                },
            );
        }
    }
}

/// Installs `window.__TAURI__.core.invoke` for trusted site pages and answers
/// it from [`ipc::Ipc`]. Commands run off the GTK main thread.
fn setup_ipc(webview: &WebView, ipc: Arc<ipc::Ipc>, restore_guard: Option<String>) {
    let Some(manager) = webview.user_content_manager() else {
        return;
    };

    let allow: Vec<String> = site_hosts::SITE_HOSTS
        .iter()
        .map(|host| format!("https://{host}/*"))
        .collect();
    let allow: Vec<&str> = allow.iter().map(String::as_str).collect();
    let shim = IPC_SHIM.replace("__ANIVOX_ORIGINS__", &site_hosts::js_origins());
    let scripts = std::iter::once(shim.as_str())
        .chain(restore_guard.as_deref())
        .chain(SITE_SCRIPTS);
    for source in scripts {
        manager.add_script(&UserScript::new(
            source,
            UserContentInjectedFrames::TopFrame,
            UserScriptInjectionTime::Start,
            &allow,
            &[],
        ));
    }

    if !manager.register_script_message_handler_with_reply(IPC_HANDLER, None) {
        eprintln!("Не удалось зарегистрировать обработчик сообщений");
        return;
    }
    let view = webview.downgrade();
    manager.connect_script_message_with_reply_received(
        Some(IPC_HANDLER),
        move |_, value, reply| {
            // The page must still be on a trusted origin when the message arrives.
            let uri = view.upgrade().and_then(|view| view.uri());
            let trusted = uri
                .as_deref()
                .and_then(|uri| Url::parse(uri).ok())
                .is_some_and(|url| site_hosts::is_site_origin(&url));
            let reply = reply.clone();
            if !trusted {
                reply.return_error_message("Недоступно для этой страницы");
                return true;
            }
            ipc.host().set_page(uri.map(|uri| uri.to_string()));
            let Some(context) = value.context() else {
                reply.return_error_message("Нет контекста JavaScript");
                return true;
            };
            let parsed = serde_json::from_str::<serde_json::Value>(&value.to_str())
                .map_err(|err| err.to_string())
                .and_then(|message| {
                    let command = message["cmd"].as_str().ok_or("Нет команды")?.to_owned();
                    Ok((command, message["args"].clone()))
                });
            let ipc = ipc.clone();
            glib::spawn_future_local(async move {
                let result = match parsed {
                    Ok((command, args)) => gio::spawn_blocking(move || ipc.handle(&command, args))
                        .await
                        .map_err(|_| "Команда завершилась аварийно".to_string())
                        .and_then(|result| result),
                    Err(err) => Err(err),
                };
                match result {
                    Ok(value) => {
                        let text = value.to_string();
                        reply.return_value(&webkit6::javascriptcore::Value::new_string(
                            &context,
                            Some(&text),
                        ));
                    }
                    Err(err) => reply.return_error_message(&err),
                }
            });
            true
        },
    );
}

/// WebKit rounds page rendering to ~60 fps on 120/240 Hz screens unless this
/// preference is off; the vblank emulation in [`vblank`] supplies the real rate.
fn disable_60fps_preference(webview: &WebView) {
    let (Some(settings), Some(features)) = (
        WebViewExt::settings(webview),
        webkit6::Settings::all_features(),
    ) else {
        return;
    };
    for feature in (0..features.length()).filter_map(|index| features.get(index)) {
        if feature.identifier().as_deref() == Some("PreferPageRenderingUpdatesNear60FPS") {
            settings.set_feature_enabled(&feature, false);
        }
    }
}

fn home_url() -> String {
    format!("https://{}/", site_hosts::SITE_HOSTS[0])
}

fn main() -> glib::ExitCode {
    // GTK's default Vulkan renderer drops WebKit to ~85 fps on NVIDIA (the GL
    // renderer reaches the monitor's rate). Must be set before GTK starts.
    if std::env::var_os("GSK_RENDERER").is_none() {
        std::env::set_var("GSK_RENDERER", "gl");
    }
    let app = gtk4::Application::builder().application_id(APP_ID).build();
    // GApplication is single-instance: a second launch re-activates the first.
    app.connect_activate(|app| {
        // The window may be hidden in the tray: show it instead of a second one.
        if let Some(window) = app.windows().into_iter().next() {
            window.present();
            return;
        }
        build_window(app);
    });
    app.run()
}

fn build_window(app: &gtk4::Application) {
    let webview = WebView::new();
    disable_60fps_preference(&webview);

    let config_dir = glib::user_config_dir().join(APP_DIR);
    let data_dir = glib::user_data_dir().join(APP_DIR);
    let session = Rc::new(session::Session::open(config_dir.join("session-gtk.json")));
    let (ui_tx, ui_rx) = async_channel::unbounded();
    let ipc = Arc::new(
        ipc::Ipc::new(ipc::GtkHost::new(
            ui_tx,
            config_dir.clone(),
            data_dir.clone(),
        ))
        .expect("не удалось запустить фоновые задачи"),
    );
    let saved = session.data();
    let (width, height) = saved.size().unwrap_or((1280, 800));
    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("Anivox")
        .default_width(width)
        .default_height(height)
        .build();
    // The site fills the window; the recovery screen and Shikimori lie above it.
    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(&webview));
    window.set_child(Some(&overlay));
    let recovery = site_recovery::install(&overlay, &webview, ipc.host().site_load());
    window.set_size_request(session::MIN_WIDTH, session::MIN_HEIGHT);
    if saved.maximized {
        window.maximize();
    }
    track_session(&window, &webview, &session);
    setup_tray(app, &window, &session);

    let header = gtk4::HeaderBar::new();
    let back = gtk4::Button::from_icon_name("go-previous-symbolic");
    let forward = gtk4::Button::from_icon_name("go-next-symbolic");
    let reload = gtk4::Button::from_icon_name("view-refresh-symbolic");
    let home = gtk4::Button::from_icon_name("go-home-symbolic");
    for button in [&back, &forward, &reload, &home] {
        header.pack_start(button);
    }
    let shikimori = shikimori::Shikimori::install(&overlay, &webview, &ipc);
    header.pack_start(&shikimori.button);
    let settings_button = settings::button(
        &window,
        &session,
        &ipc,
        config_dir.clone(),
        data_dir.join("logs"),
    );
    header.pack_end(&settings_button);
    let connection_button = connection::button(&ipc);
    header.pack_end(&connection_button);
    let status = gtk4::Label::new(None);
    status.set_visible(false);
    header.pack_end(&status);
    window.set_titlebar(Some(&header));
    let shell = Rc::new(Shell {
        window: window.clone(),
        header: header.clone(),
        status,
        status_generation: Cell::new(0),
        recovery,
        _shikimori: shikimori,
    });
    glib::spawn_future_local(async move {
        while let Ok(event) = ui_rx.recv().await {
            shell.handle(event);
        }
    });

    back.connect_clicked(glib::clone!(
        #[weak]
        webview,
        move |_| webview.go_back()
    ));
    forward.connect_clicked(glib::clone!(
        #[weak]
        webview,
        move |_| webview.go_forward()
    ));
    reload.connect_clicked(glib::clone!(
        #[weak]
        webview,
        move |_| webview.reload()
    ));
    home.connect_clicked(glib::clone!(
        #[weak]
        webview,
        move |_| webview.load_uri(&home_url())
    ));

    // Keep the back/forward buttons in sync with the history.
    let sync = glib::clone!(
        #[weak]
        back,
        #[weak]
        forward,
        move |view: &WebView| {
            back.set_sensitive(view.can_go_back());
            forward.set_sensitive(view.can_go_forward());
        }
    );
    webview.connect_uri_notify(sync.clone());
    webview.connect_load_changed(move |view, _| sync(view));
    back.set_sensitive(false);
    forward.set_sensitive(false);

    webview.connect_title_notify(glib::clone!(
        #[weak]
        window,
        move |view| {
            let title = view.title().filter(|title| !title.is_empty());
            window.set_title(Some(title.as_deref().unwrap_or("Anivox")));
        }
    ));

    // Links to foreign hosts open in the system browser; the site and its
    // embedded players keep working inside the view.
    webview.connect_decide_policy(|_, decision, kind| {
        if kind != PolicyDecisionType::NavigationAction {
            return false;
        }
        let Some(nav) = decision.downcast_ref::<NavigationPolicyDecision>() else {
            return false;
        };
        let Some(action) = nav.navigation_action() else {
            return false;
        };
        if action.navigation_type() != NavigationType::LinkClicked {
            return false;
        }
        let Some(uri) = action.request().and_then(|request| request.uri()) else {
            return false;
        };
        let trusted = Url::parse(&uri).is_ok_and(|url| site_hosts::is_site_origin(&url));
        if trusted {
            return false;
        }
        let _ = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE);
        decision.ignore();
        true
    });

    let startup = saved.startup_url();
    setup_ipc(&webview, ipc, session::restore_guard_script(&startup));
    webview.load_uri(startup.as_str());
    window.present();
}

/// Tray icon with "Открыть / Сворачивать в трей / Выход". While the icon is
/// shown and the preference is on, the close button hides the window; the
/// application exits only from the tray menu.
fn setup_tray(
    app: &gtk4::Application,
    window: &gtk4::ApplicationWindow,
    session: &Rc<session::Session>,
) {
    let path = glib::user_config_dir().join(APP_DIR).join("tray.json");
    let minimize = Rc::new(Cell::new(tray::load_preference(&path)));
    let online = Rc::new(Cell::new(false));
    let handle: Rc<std::cell::RefCell<Option<tray::TrayHandle>>> = Rc::default();

    window.connect_close_request(glib::clone!(
        #[strong]
        minimize,
        #[strong]
        online,
        move |window| {
            if online.get() && minimize.get() {
                window.set_visible(false);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    ));

    let flush = glib::clone!(
        #[strong]
        session,
        move || {
            if let Err(err) = session.flush() {
                eprintln!("Не удалось сохранить сеанс: {err}");
            }
        }
    );
    tray::start(
        minimize.get(),
        glib::clone!(
            #[strong]
            handle,
            #[strong]
            online,
            move |result| match result {
                Ok(tray) => {
                    online.set(true);
                    *handle.borrow_mut() = Some(tray);
                }
                Err(err) => eprintln!("Значок в трее недоступен: {err}"),
            }
        ),
        glib::clone!(
            #[weak]
            app,
            #[weak]
            window,
            move |event| match event {
                tray::TrayEvent::Toggle if window.is_visible() => {
                    flush();
                    window.set_visible(false);
                }
                tray::TrayEvent::Toggle | tray::TrayEvent::Show => window.present(),
                tray::TrayEvent::ToggleMinimize => {
                    let enabled = !minimize.get();
                    match anivox_core::tray_settings::save(&path, enabled) {
                        Ok(()) => minimize.set(enabled),
                        Err(err) => eprintln!("Не удалось сохранить настройку трея: {err}"),
                    }
                    let current = minimize.get();
                    if let Some(tray) = handle.borrow().as_ref() {
                        tray.update(|tray| tray.minimize_to_tray = current);
                    }
                }
                tray::TrayEvent::Quit => {
                    flush();
                    app.quit();
                }
                tray::TrayEvent::Online(available) => {
                    online.set(available);
                    // Never leave a hidden window without a way back.
                    if !available {
                        window.present();
                    }
                }
            }
        ),
    );
}

/// Keeps the session file up to date: the page on every navigation (SPA
/// navigation included), the window size after it has settled for a moment.
fn track_session(
    window: &gtk4::ApplicationWindow,
    webview: &WebView,
    session: &Rc<session::Session>,
) {
    let observe = glib::clone!(
        #[strong]
        session,
        move |window: &gtk4::ApplicationWindow| {
            let (width, height) = window.default_size();
            session.observe_window(width, height, window.is_maximized(), window.is_fullscreen());
            let generation = session.generation();
            let session = session.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || {
                if session.generation() == generation {
                    if let Err(err) = session.flush() {
                        eprintln!("Не удалось сохранить состояние окна: {err}");
                    }
                }
            });
        }
    );
    window.connect_default_width_notify(observe.clone());
    window.connect_default_height_notify(observe.clone());
    window.connect_maximized_notify(observe);
    webview.connect_uri_notify(glib::clone!(
        #[strong]
        session,
        move |view| {
            if let Some(uri) = view.uri() {
                session.set_last_page(&uri);
            }
        }
    ));
    window.connect_close_request(glib::clone!(
        #[strong]
        session,
        move |_| {
            if let Err(err) = session.flush() {
                eprintln!("Не удалось сохранить сеанс: {err}");
            }
            glib::Propagation::Proceed
        }
    ));
}
