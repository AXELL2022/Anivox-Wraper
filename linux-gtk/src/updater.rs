//! Update check and the changelog review popover.
//!
//! The release manifest (`latest.json`, the same one the Tauri updater reads)
//! carries the version and the notes from CHANGELOG. A pacman package is not
//! replaced in place: after review the button opens the release page for a
//! manual update. Before that the version is checked again; if it changed, the
//! new notes are shown and need a fresh confirmation.

use crate::ipc::Ipc;
use anivox_core::updater::{UpdateInfo, RELEASE_PAGE};
use gtk4::prelude::*;
use gtk4::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

struct Review {
    button: gtk4::MenuButton,
    title: gtk4::Label,
    notes: gtk4::TextView,
    status: gtk4::Label,
    open: gtk4::Button,
    retry: gtk4::Button,
    /// The version whose notes are on screen: only it can be confirmed.
    reviewed: RefCell<Option<UpdateInfo>>,
    ipc: Arc<Ipc>,
}

impl Review {
    fn show(&self, info: Option<UpdateInfo>) {
        match &info {
            Some(info) => {
                self.button.set_label(&format!("v{}", info.version));
                self.button
                    .set_tooltip_text(Some(&format!("Доступно обновление v{}", info.version)));
                self.title.set_text(&format!("Anivox v{}", info.version));
                let notes = if info.notes.trim().is_empty() {
                    "Описание изменений не указано."
                } else {
                    info.notes.trim()
                };
                self.notes.buffer().set_text(notes);
            }
            None => self.title.set_text("Обновление Anivox"),
        }
        self.open.set_sensitive(info.is_some());
        self.button
            .set_visible(info.is_some() || self.button.is_visible());
        *self.reviewed.borrow_mut() = info;
    }

    fn check(
        self: &Rc<Self>,
        then: impl FnOnce(&Rc<Self>, Result<Option<UpdateInfo>, String>) + 'static,
    ) {
        self.open.set_sensitive(false);
        self.retry.set_sensitive(false);
        let ipc = self.ipc.clone();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result =
                gio::spawn_blocking(move || ipc.block_on(anivox_core::updater::check(CURRENT)))
                    .await
                    .unwrap_or_else(|_| Err("Проверка завершилась аварийно".into()));
            this.retry.set_sensitive(true);
            this.open.set_sensitive(this.reviewed.borrow().is_some());
            then(&this, result);
        });
    }

    /// Re-checks right before acting so an unreviewed version is never opened.
    fn confirm(self: &Rc<Self>) {
        let Some(expected) = self.reviewed.borrow().clone() else {
            return;
        };
        self.status.set_text("Проверка версии…");
        self.check(move |this, result| match result {
            Ok(Some(info)) if info.version == expected.version => {
                match gio::AppInfo::launch_default_for_uri(
                    RELEASE_PAGE,
                    gio::AppLaunchContext::NONE,
                ) {
                    Ok(()) => this.status.set_text(
                        "Открыта страница выпуска. Скачайте пакет и установите его \
                         (pacman -U), затем перезапустите Anivox.",
                    ),
                    Err(err) => this
                        .status
                        .set_text(&format!("Не удалось открыть браузер: {err}")),
                }
            }
            Ok(Some(info)) => {
                this.show(Some(info));
                this.status.set_text(
                    "Вышла другая версия. Ознакомьтесь с изменениями и подтвердите ещё раз.",
                );
            }
            Ok(None) => {
                this.show(None);
                this.status.set_text("Обновление больше не доступно.");
            }
            Err(err) => this.status.set_text(&err),
        });
    }
}

/// Header button that appears when a newer version is published.
pub fn button(ipc: &Arc<Ipc>) -> gtk4::MenuButton {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_width_request(440);
    let title = gtk4::Label::new(Some("Обновление Anivox"));
    title.add_css_class("title-4");
    title.set_xalign(0.0);
    content.append(&title);
    let current = gtk4::Label::new(Some(&format!("Установлена версия v{CURRENT}")));
    current.add_css_class("dim-label");
    current.set_xalign(0.0);
    content.append(&current);
    let notes = gtk4::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(gtk4::WrapMode::WordChar)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    let scroller = gtk4::ScrolledWindow::builder()
        .child(&notes)
        .min_content_height(220)
        .max_content_height(360)
        .propagate_natural_height(true)
        .build();
    scroller.add_css_class("card");
    content.append(&scroller);
    let status = gtk4::Label::new(Some("Перед установкой ознакомьтесь с изменениями."));
    status.set_wrap(true);
    status.set_xalign(0.0);
    content.append(&status);
    let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let open = gtk4::Button::with_label("Открыть страницу обновления");
    open.add_css_class("suggested-action");
    let retry = gtk4::Button::with_label("Проверить снова");
    let later = gtk4::Button::with_label("Позже");
    actions.append(&open);
    actions.append(&retry);
    actions.append(&later);
    content.append(&actions);

    let popover = gtk4::Popover::builder().child(&content).build();
    let button = gtk4::MenuButton::builder()
        .label("Обновление")
        .popover(&popover)
        .visible(false)
        .build();
    button.add_css_class("suggested-action");

    let review = Rc::new(Review {
        button: button.clone(),
        title,
        notes,
        status,
        open: open.clone(),
        retry: retry.clone(),
        reviewed: RefCell::new(None),
        ipc: ipc.clone(),
    });
    open.connect_clicked(glib::clone!(
        #[strong]
        review,
        move |_| review.confirm()
    ));
    retry.connect_clicked(glib::clone!(
        #[strong]
        review,
        move |_| {
            review.status.set_text("Проверка обновлений…");
            review.check(|this, result| match result {
                Ok(info) => {
                    let found = info.is_some();
                    this.show(info);
                    this.status.set_text(if found {
                        "Перед установкой ознакомьтесь с изменениями."
                    } else {
                        "У вас последняя версия."
                    });
                }
                Err(err) => this.status.set_text(&err),
            });
        }
    ));
    later.connect_clicked(glib::clone!(
        #[weak]
        popover,
        move |_| popover.popdown()
    ));

    // Background check shortly after launch, like the Tauri build.
    glib::timeout_add_local_once(
        Duration::from_secs(3),
        glib::clone!(
            #[strong]
            review,
            move || {
                review.check(|this, result| match result {
                    Ok(info) => this.show(info),
                    Err(err) => eprintln!("{err}"),
                })
            }
        ),
    );
    button
}
