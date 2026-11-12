//! Loading watchdog and the native recovery screen over the site.
//!
//! A navigation starts a new attempt (generation). `site-lifecycle.js` reports
//! when the site's app has really mounted; after 25 s without that the screen
//! offers to retry, open the mirror or the home page, or keep waiting. A timeout
//! is not proof of a network failure, so a slow page may still finish later.

use anivox_core::site_hosts;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use url::Url;
use webkit6::prelude::*;
use webkit6::WebView;

const WATCHDOG: Duration = Duration::from_secs(25);
pub const MIRROR_URL: &str = "https://anivox.me/";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Loading,
    Ready,
    Timeout,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadInfo {
    pub phase: Phase,
    pub detail: Option<String>,
    pub generation: u64,
}

impl Default for LoadInfo {
    fn default() -> Self {
        Self {
            phase: Phase::Loading,
            detail: None,
            generation: 0,
        }
    }
}

impl LoadInfo {
    pub fn begin(&mut self) -> u64 {
        self.generation += 1;
        self.phase = Phase::Loading;
        self.detail = None;
        self.generation
    }

    /// Only the current attempt can finish; a late report of an older one is ignored.
    pub fn finish(&mut self, generation: u64) -> bool {
        if self.generation != generation {
            return false;
        }
        self.phase = Phase::Ready;
        self.detail = None;
        true
    }

    pub fn timeout(&mut self, generation: u64) -> bool {
        if self.generation != generation || self.phase != Phase::Loading {
            return false;
        }
        self.phase = Phase::Timeout;
        true
    }

    pub fn fail(&mut self, detail: String) {
        self.phase = Phase::Error;
        self.detail = Some(detail);
    }
}

pub type SharedLoad = Arc<Mutex<LoadInfo>>;

fn lock(state: &SharedLoad) -> std::sync::MutexGuard<'_, LoadInfo> {
    state.lock().unwrap_or_else(|err| err.into_inner())
}

/// A finished real HTTP(S) document on a foreign host (e.g. a login page) also
/// ends the wait: `site-lifecycle.js` runs only on the site itself.
pub fn is_external_document(uri: &str) -> bool {
    Url::parse(uri).is_ok_and(|url| {
        matches!(url.scheme(), "https" | "http")
            && url
                .host_str()
                .is_some_and(|host| !site_hosts::is_site_host(host))
    })
}

/// Validates a `site_document_state` report against the page the shell shows.
/// Returns whether it may finish the given attempt.
pub fn document_report_matches(page: &str, href: &str) -> Result<bool, String> {
    let page = Url::parse(page).map_err(|err| err.to_string())?;
    let url = Url::parse(href).map_err(|err| err.to_string())?;
    if !site_hosts::is_site_origin(&page) || url.origin() != page.origin() {
        return Err("Недопустимый адрес сайта".into());
    }
    // A late document-ready from a previous navigation must not hide recovery.
    Ok(url.path() == page.path() && url.query() == page.query())
}

const CSS: &str = "
.anivox-recovery { background: radial-gradient(ellipse at 22% 12%, alpha(#6550b3, 0.15), transparent 52%), #101018; color: #f3f2ff; }
.anivox-recovery .brand { font-weight: 750; letter-spacing: 4px; color: #d3cee8; font-size: 13px; }
.anivox-recovery .card { background: alpha(#1a1923, 0.91); border: 1px solid alpha(white, 0.07); border-radius: 24px; padding: 34px 40px 28px; }
.anivox-recovery .eyebrow { color: #b5a3ee; font-size: 10px; font-weight: 700; letter-spacing: 3px; }
.anivox-recovery .title { font-size: 26px; font-weight: 650; }
.anivox-recovery .description { color: #b3afc3; font-size: 14px; }
.anivox-recovery .detail { color: #cbc5df; font-size: 12px; background: alpha(#0f0e16, 0.4); border-radius: 10px; padding: 10px 12px; }
.anivox-recovery button { min-height: 38px; padding: 4px 16px; border-radius: 10px; }
.anivox-recovery button.suggested-action { background: #ae98fa; color: #201635; }
.anivox-recovery .footnote { color: #837d96; font-size: 11px; }
";

pub struct Recovery {
    root: gtk4::Box,
    spinner: gtk4::Spinner,
    title: gtk4::Label,
    description: gtk4::Label,
    detail: gtk4::Label,
    state: SharedLoad,
    dismissed: Cell<Option<u64>>,
}

impl Recovery {
    fn render(&self) {
        let info = lock(&self.state).clone();
        let hidden = info.phase == Phase::Ready || self.dismissed.get() == Some(info.generation);
        self.root.set_visible(!hidden);
        self.spinner.set_spinning(info.phase == Phase::Loading);
        self.spinner.set_visible(info.phase == Phase::Loading);
        let (title, description) = match info.phase {
            Phase::Loading => (
                "Загрузка Anivox…",
                "Подключаемся к сайту. Это может занять немного времени.",
            ),
            Phase::Ready => ("Anivox готов", "Возвращаемся к просмотру…"),
            Phase::Timeout => (
                "Сайт не ответил вовремя",
                "Сайт или сеть могут работать медленно. Можно повторить загрузку, \
                 открыть зеркало или главную, либо продолжить ожидание.",
            ),
            Phase::Error => (
                "Не удалось загрузить Anivox",
                "При загрузке страницы произошла ошибка. Попробуйте ещё раз, \
                 откройте зеркало или перейдите на главную.",
            ),
        };
        self.title.set_text(title);
        self.description.set_text(description);
        self.detail.set_text(info.detail.as_deref().unwrap_or(""));
        self.detail.set_visible(info.detail.is_some());
    }

    /// A new navigation of the site view: show the loading screen and arm the watchdog.
    pub fn begin(self: &Rc<Self>) {
        let generation = lock(&self.state).begin();
        self.render();
        let recovery = Rc::downgrade(self);
        glib::timeout_add_local_once(WATCHDOG, move || {
            if let Some(recovery) = recovery.upgrade() {
                if lock(&recovery.state).timeout(generation) {
                    recovery.render();
                }
            }
        });
    }

    pub fn finish(&self, generation: u64) {
        if lock(&self.state).finish(generation) {
            self.render();
        }
    }

    pub fn finish_current(&self) {
        let generation = lock(&self.state).generation;
        self.finish(generation);
    }

    pub fn fail(&self, detail: String) {
        lock(&self.state).fail(detail);
        self.render();
    }
}

fn label(text: &str, class: &str) -> gtk4::Label {
    let label = gtk4::Label::new(Some(text));
    label.add_css_class(class);
    label.set_wrap(true);
    label.set_justify(gtk4::Justification::Center);
    label
}

/// Adds the recovery screen over `webview` inside `overlay` and wires the
/// watchdog to the view's load signals.
pub fn install(overlay: &gtk4::Overlay, webview: &WebView, state: SharedLoad) -> Rc<Recovery> {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(CSS);
    gtk4::style_context_add_provider_for_display(
        &WidgetExt::display(overlay),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 22);
    root.add_css_class("anivox-recovery");
    root.set_valign(gtk4::Align::Fill);
    root.set_halign(gtk4::Align::Fill);
    let column = gtk4::Box::new(gtk4::Orientation::Vertical, 22);
    column.set_valign(gtk4::Align::Center);
    column.set_halign(gtk4::Align::Center);
    column.set_vexpand(true);
    column.set_margin_start(16);
    column.set_margin_end(16);
    root.append(&column);
    column.append(&label("ANIVOX", "brand"));

    let card = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    card.add_css_class("card");
    card.set_width_request(360);
    let spinner = gtk4::Spinner::new();
    spinner.set_size_request(48, 48);
    card.append(&spinner);
    card.append(&label("ВАШ МИР АНИМЕ", "eyebrow"));
    let title = label("", "title");
    let description = label("", "description");
    description.set_max_width_chars(52);
    let detail = label("", "detail");
    detail.set_selectable(true);
    detail.set_justify(gtk4::Justification::Left);
    card.append(&title);
    card.append(&description);
    card.append(&detail);

    let actions = gtk4::FlowBox::new();
    actions.set_selection_mode(gtk4::SelectionMode::None);
    actions.set_max_children_per_line(3);
    actions.set_homogeneous(false);
    actions.set_halign(gtk4::Align::Center);
    actions.set_margin_top(12);
    let retry = gtk4::Button::with_label("Повторить загрузку");
    retry.add_css_class("suggested-action");
    let mirror = gtk4::Button::with_label("На зеркало");
    let home = gtk4::Button::with_label("На главную");
    for button in [&retry, &mirror, &home] {
        actions.insert(button, -1);
    }
    card.append(&actions);
    let wait = gtk4::Button::with_label("Продолжить ожидание");
    wait.add_css_class("flat");
    wait.set_halign(gtk4::Align::Center);
    card.append(&wait);
    column.append(&card);
    column.append(&label(
        "Anivox Desktop · Немного терпения — и можно смотреть",
        "footnote",
    ));
    overlay.add_overlay(&root);

    let recovery = Rc::new(Recovery {
        root,
        spinner,
        title,
        description,
        detail,
        state,
        dismissed: Cell::new(None),
    });

    retry.connect_clicked(glib::clone!(
        #[weak]
        webview,
        move |_| {
            let current = webview
                .uri()
                .filter(|uri| uri.starts_with("https://") || uri.starts_with("http://"));
            match current {
                Some(uri) => webview.load_uri(&uri),
                None => webview.load_uri(&crate::home_url()),
            }
        }
    ));
    mirror.connect_clicked(glib::clone!(
        #[weak]
        webview,
        move |_| webview.load_uri(MIRROR_URL)
    ));
    home.connect_clicked(glib::clone!(
        #[weak]
        webview,
        move |_| webview.load_uri(&crate::home_url())
    ));
    wait.connect_clicked(glib::clone!(
        #[weak]
        recovery,
        move |_| {
            let generation = lock(&recovery.state).generation;
            recovery.dismissed.set(Some(generation));
            recovery.render();
        }
    ));

    webview.connect_load_changed(glib::clone!(
        #[weak]
        recovery,
        move |view, event| match event {
            webkit6::LoadEvent::Started => recovery.begin(),
            webkit6::LoadEvent::Finished
                if view.uri().is_some_and(|uri| is_external_document(&uri)) =>
            {
                recovery.finish_current();
            }
            _ => {}
        }
    ));
    webview.connect_load_failed(glib::clone!(
        #[weak]
        recovery,
        #[upgrade_or]
        false,
        move |_, _, uri, error| {
            // A newer navigation or a download cancels the old load: not a failure.
            if error.matches(webkit6::NetworkError::Cancelled)
                || error.matches(webkit6::PolicyError::FrameLoadInterruptedByPolicyChange)
            {
                return false;
            }
            // Handled: WebKit's own error page would count as a finished
            // external document and hide the recovery screen.
            recovery.fail(format!("{uri}\n{}", error.message()));
            true
        }
    ));
    recovery.render();
    recovery
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_reload_completion_and_watchdog_cannot_finish_current_attempt() {
        let mut state = LoadInfo::default();
        state.begin();
        let current = state.begin();
        assert!(!state.finish(current - 1));
        assert!(!state.timeout(current - 1));
        assert_eq!(state.phase, Phase::Loading);
        assert!(state.finish(current));
        assert!(!state.timeout(current));
        assert_eq!(state.phase, Phase::Ready);
    }

    #[test]
    fn slow_document_can_finish_after_timeout_without_a_reload() {
        let mut state = LoadInfo::default();
        let current = state.begin();
        assert!(state.timeout(current));
        assert_eq!(state.phase, Phase::Timeout);
        assert!(state.finish(current));
        assert_eq!(state.phase, Phase::Ready);
    }

    #[test]
    fn blank_engine_and_site_documents_cannot_dismiss_loading() {
        for value in [
            "about:blank",
            "data:text/html,",
            "https://anivox.fun/",
            "https://anivox.me/",
        ] {
            assert!(!is_external_document(value), "{value}");
        }
        assert!(is_external_document("https://accounts.google.com/"));
    }

    #[test]
    fn document_reports_must_match_the_shown_page() {
        let page = "https://anivox.fun/anime/1-x?episode=2";
        assert!(document_report_matches(page, page).unwrap());
        assert!(!document_report_matches(page, "https://anivox.fun/").unwrap());
        assert!(document_report_matches(page, "https://anivox.me/anime/1-x").is_err());
        assert!(document_report_matches("https://evil.test/", "https://evil.test/").is_err());
    }
}
