//! Update check and the changelog review popover.
//!
//! The release manifest (`latest.json`, the same one the Tauri updater reads)
//! carries the version and the notes from CHANGELOG. A pacman package is not
//! replaced in place: after review the button opens the release page for a
//! manual update. Before that the version is checked again; if it changed, the
//! new notes are shown and need a fresh confirmation.

use crate::ipc::Ipc;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

const MANIFEST_URL: &str =
    "https://github.com/AXELL2022/Anivox-Wraper/releases/latest/download/latest.json";
pub const RELEASE_PAGE: &str = "https://github.com/AXELL2022/Anivox-Wraper/releases/latest";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct UpdateInfo {
    pub version: String,
    #[serde(default)]
    pub notes: String,
}

fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let core = value.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// True only for a well-formed version strictly newer than the running one.
pub fn is_newer(remote: &str, current: &str) -> bool {
    match (parse_version(remote), parse_version(current)) {
        (Some(remote), Some(current)) => remote > current,
        _ => false,
    }
}

/// `Some` when the manifest announces a newer version.
pub fn available(manifest: &[u8], current: &str) -> Result<Option<UpdateInfo>, String> {
    let mut info: UpdateInfo = serde_json::from_slice(manifest)
        .map_err(|err| format!("Некорректный манифест обновления: {err}"))?;
    info.version = info.version.trim().trim_start_matches('v').to_owned();
    Ok(is_newer(&info.version, current).then_some(info))
}

pub async fn check() -> Result<Option<UpdateInfo>, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .user_agent(concat!("Anivox/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|err| err.to_string())?;
    let bytes = client
        .get(MANIFEST_URL)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|err| format!("Не удалось проверить обновления: {err}"))?
        .bytes()
        .await
        .map_err(|err| err.to_string())?;
    available(&bytes, CURRENT)
}

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
            let result = gio::spawn_blocking(move || ipc.block_on(check()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_strictly_newer_versions_are_offered() {
        assert!(is_newer("0.4.4", "0.4.3"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("0.4.3", "0.4.3"));
        assert!(!is_newer("0.4.2", "0.4.3"));
        assert!(!is_newer("garbage", "0.4.3"));
        assert!(!is_newer("1.2", "0.4.3"));
    }

    #[test]
    fn manifest_notes_come_with_the_version() {
        let manifest =
            br###"{"version": "v9.0.0", "notes": "## [9.0.0]\n* new", "platforms": {}}"###;
        let info = available(manifest, "0.4.3").unwrap().unwrap();
        assert_eq!(info.version, "9.0.0");
        assert!(info.notes.contains("* new"));
        let same = br#"{"version": "0.4.3", "notes": ""}"#;
        assert_eq!(available(same, "0.4.3").unwrap(), None);
        assert!(available(b"<html>", "0.4.3").is_err());
    }
}
