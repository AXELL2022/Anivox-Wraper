//! Native settings popover: startup page, MPV path, embedded mirroring and
//! clearing saved positions.
//!
//! The panel is a GTK widget of the shell itself, so remote pages have no IPC
//! command for any of this: only this panel can choose or save the MPV path.

use crate::ipc::Ipc;
use crate::session::{Session, StartupPage};
use anivox_core::player_settings::{load_player_settings, save_player_settings, PlayerSettings};
use gtk4::prelude::*;
use gtk4::{gio, glib};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

struct Widgets {
    startup: gtk4::DropDown,
    path: gtk4::Entry,
    mirror: gtk4::CheckButton,
    status: gtk4::Label,
    session_status: gtk4::Label,
}

fn heading(text: &str) -> gtk4::Label {
    let label = gtk4::Label::new(Some(text));
    label.add_css_class("heading");
    label.set_xalign(0.0);
    label
}

fn hint(text: &str) -> gtk4::Label {
    let label = gtk4::Label::new(Some(text));
    label.add_css_class("dim-label");
    label.add_css_class("caption");
    label.set_wrap(true);
    label.set_max_width_chars(48);
    label.set_xalign(0.0);
    label
}

fn startup_index(page: StartupPage) -> u32 {
    match page {
        StartupPage::LastPage => 0,
        StartupPage::Home => 1,
    }
}

/// Gear button in the header bar that opens the settings popover.
pub fn button(
    window: &gtk4::ApplicationWindow,
    session: &Rc<Session>,
    ipc: &Arc<Ipc>,
    config_dir: PathBuf,
    logs_dir: PathBuf,
) -> gtk4::MenuButton {
    let settings_path = config_dir.join("settings.json");
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_width_request(380);

    let title = gtk4::Label::new(Some("Настройки Anivox"));
    title.add_css_class("title-4");
    title.set_xalign(0.0);
    content.append(&title);

    content.append(&heading("Приложение"));
    let startup_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let startup_label = gtk4::Label::new(Some("При запуске открывать"));
    startup_label.set_hexpand(true);
    startup_label.set_xalign(0.0);
    let startup = gtk4::DropDown::from_strings(&["Последнюю страницу Anivox", "Главную страницу"]);
    startup_row.append(&startup_label);
    startup_row.append(&startup);
    content.append(&startup_row);
    content.append(&hint(
        "Размер и развёрнутое состояние окна сохраняются автоматически. \
         Восстановленная страница не запускает видео до взаимодействия с ней.",
    ));
    let session_status = hint("");
    content.append(&session_status);

    content.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
    content.append(&heading("MPV"));
    let path_label = gtk4::Label::new(Some("Путь к исполняемому файлу MPV"));
    path_label.set_xalign(0.0);
    content.append(&path_label);
    let path_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let path = gtk4::Entry::builder()
        .placeholder_text("mpv из PATH")
        .hexpand(true)
        .build();
    let browse = gtk4::Button::with_label("Обзор…");
    path_row.append(&path);
    path_row.append(&browse);
    content.append(&path_row);
    content.append(&hint(
        "Оставьте поле пустым для поиска MPV в PATH. Выбранный путь используется \
         при следующем запуске видео.",
    ));
    let mirror =
        gtk4::CheckButton::with_label("Продолжать воспроизведение на сайте при просмотре в MPV");
    if let Some(label) = mirror.child().and_downcast::<gtk4::Label>() {
        label.set_wrap(true);
    }
    content.append(&mirror);
    content.append(&hint(
        "После запуска MPV встроенный плеер работает без звука на минимальном качестве \
         и следует его паузе и позиции. Расход трафика и нагрузка увеличатся.",
    ));

    let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let save = gtk4::Button::with_label("Сохранить");
    save.add_css_class("suggested-action");
    let reset = gtk4::Button::with_label("Сбросить путь");
    actions.append(&save);
    actions.append(&reset);
    content.append(&actions);
    let clear = gtk4::Button::with_label("Очистить сохранённые позиции");
    clear.set_halign(gtk4::Align::Start);
    content.append(&clear);

    content.append(&hint("Журналы MPV сохраняются в папку данных Anivox:"));
    let logs = gtk4::Label::new(Some(&logs_dir.to_string_lossy()));
    logs.set_selectable(true);
    logs.set_wrap(true);
    logs.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    logs.set_xalign(0.0);
    logs.add_css_class("caption");
    content.append(&logs);
    let status = gtk4::Label::new(None);
    status.set_wrap(true);
    status.set_xalign(0.0);
    content.append(&status);

    let popover = gtk4::Popover::builder().child(&content).build();
    let button = gtk4::MenuButton::builder()
        .icon_name("emblem-system-symbolic")
        .tooltip_text("Настройки Anivox")
        .popover(&popover)
        .build();

    let widgets = Rc::new(Widgets {
        startup,
        path,
        mirror,
        status,
        session_status,
    });
    // An unsaved path picked in the file dialog survives reopening the popover.
    let dirty = Rc::new(Cell::new(false));
    // Ignore the dropdown signal while values are filled in programmatically.
    let loading = Rc::new(Cell::new(false));

    let reload = glib::clone!(
        #[strong]
        widgets,
        #[strong]
        session,
        #[strong]
        dirty,
        #[strong]
        loading,
        #[strong]
        settings_path,
        move || {
            loading.set(true);
            widgets
                .startup
                .set_selected(startup_index(session.data().startup_page));
            loading.set(false);
            widgets.session_status.set_text("");
            if dirty.get() {
                return;
            }
            match load_player_settings(&settings_path) {
                Ok(settings) => {
                    widgets
                        .path
                        .set_text(settings.mpv_path.as_deref().unwrap_or(""));
                    widgets.mirror.set_active(settings.mirror_embedded);
                    widgets.status.set_text("");
                }
                Err(err) => widgets.status.set_text(&err),
            }
        }
    );
    reload();
    popover.connect_show(move |_| reload());

    widgets.startup.connect_selected_notify(glib::clone!(
        #[strong]
        session,
        #[strong]
        loading,
        #[weak(rename_to = status)]
        widgets.session_status,
        move |dropdown| {
            if loading.get() {
                return;
            }
            let page = if dropdown.selected() == 1 {
                StartupPage::Home
            } else {
                StartupPage::LastPage
            };
            match session.set_startup_page(page) {
                Ok(()) => status.set_text("Вариант запуска сохранён."),
                Err(err) => status.set_text(&format!("Не удалось сохранить запуск: {err}")),
            }
        }
    ));

    widgets.path.connect_changed(glib::clone!(
        #[strong]
        dirty,
        move |_| dirty.set(true)
    ));
    widgets.mirror.connect_toggled(glib::clone!(
        #[strong]
        dirty,
        move |_| dirty.set(true)
    ));

    browse.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        popover,
        #[strong]
        widgets,
        move |_| {
            let dialog = gtk4::FileDialog::builder()
                .title("Выберите MPV")
                .modal(true)
                .build();
            let widgets = widgets.clone();
            dialog.open(
                Some(&window),
                gio::Cancellable::NONE,
                glib::clone!(
                    #[weak]
                    popover,
                    move |result| {
                        if let Some(path) = result.ok().and_then(|file| file.path()) {
                            widgets.path.set_text(&path.to_string_lossy());
                            widgets
                                .status
                                .set_text("Нажмите «Сохранить», чтобы применить путь.");
                        }
                        popover.popup();
                    }
                ),
            );
            popover.popdown();
        }
    ));

    reset.connect_clicked(glib::clone!(
        #[strong]
        widgets,
        move |_| {
            widgets.path.set_text("");
            widgets
                .status
                .set_text("Нажмите «Сохранить», чтобы использовать MPV из PATH.");
        }
    ));

    save.connect_clicked(glib::clone!(
        #[strong]
        widgets,
        #[strong]
        dirty,
        move |_| {
            let path = widgets.path.text().trim().to_owned();
            let settings = PlayerSettings {
                mpv_path: (!path.is_empty()).then_some(path),
                mirror_embedded: widgets.mirror.is_active(),
            };
            match save_player_settings(&settings_path, &settings) {
                Ok(()) => {
                    dirty.set(false);
                    widgets.status.set_text("Настройки сохранены.");
                }
                Err(err) => widgets.status.set_text(&err),
            }
        }
    ));

    clear.connect_clicked(glib::clone!(
        #[strong]
        widgets,
        #[strong]
        ipc,
        move |button| {
            button.set_sensitive(false);
            let ipc = ipc.clone();
            let widgets = widgets.clone();
            let button = button.clone();
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || ipc.clear_playback_history())
                    .await
                    .unwrap_or_else(|_| Err("Операция завершилась аварийно".into()));
                widgets.status.set_text(&match result {
                    Ok(()) => "Сохранённые позиции удалены.".to_owned(),
                    Err(err) => err,
                });
                button.set_sensitive(true);
            });
        }
    ));
    button
}
