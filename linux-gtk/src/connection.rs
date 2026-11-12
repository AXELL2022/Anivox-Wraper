//! Connection check popover: is it the user's internet or the video server?
//!
//! `segment-monitor.js` passively reports how video segments download
//! (`report_segment_stats`); the popover compares that with a download from
//! speed.cloudflare.com. The verdict logic lives in `anivox_core::connection`.

use crate::ipc::{Ipc, USER_AGENT};
use anivox_core::connection::{assess, fresh, measure_internet, now_ms, Kind, NetResult};
use anivox_core::segments::SegmentStats;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

fn paint(label: &gtk4::Label, kind: Kind) {
    for class in ["success", "error", "warning", "dim-label"] {
        label.remove_css_class(class);
    }
    label.add_css_class(match kind {
        Kind::Good => "success",
        Kind::Bad => "error",
        Kind::Warn => "warning",
        Kind::Pending => "dim-label",
    });
}

struct Panel {
    internet: gtk4::Label,
    video: gtk4::Label,
    detail: gtk4::Label,
    verdict_title: gtk4::Label,
    verdict_text: gtk4::Label,
    recheck: gtk4::Button,
    net: Cell<Option<NetResult>>,
    measuring: Cell<bool>,
    seg: RefCell<Option<SegmentStats>>,
    /// Bumped on every show/hide so a late measurement of a closed popover is dropped.
    epoch: Cell<u64>,
}

impl Panel {
    fn render(&self) {
        let seg = self.seg.borrow();
        let view = assess(self.net.get(), self.measuring.get(), seg.as_ref());
        if let Some(internet) = view.internet {
            self.internet.set_text(&internet.text);
            paint(&self.internet, internet.kind);
        }
        self.video.set_text(&view.video.text);
        paint(&self.video, view.video.kind);
        self.detail.set_visible(view.detail.is_some());
        self.detail.set_text(view.detail.as_deref().unwrap_or(""));
        let verdict = view.verdict;
        self.verdict_title.set_text(&verdict.title);
        paint(&self.verdict_title, verdict.kind);
        self.verdict_text.set_text(&verdict.text);
    }
}

fn measure(panel: &Rc<Panel>, ipc: &Arc<Ipc>) {
    if panel.measuring.get() {
        return;
    }
    panel.measuring.set(true);
    panel.recheck.set_sensitive(false);
    panel.render();
    let epoch = panel.epoch.get();
    let ipc = ipc.clone();
    let panel = panel.clone();
    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(move || ipc.block_on(measure_internet(USER_AGENT)))
            .await
            .unwrap_or(NetResult {
                ok: false,
                mbps: 0.0,
            });
        panel.measuring.set(false);
        panel.recheck.set_sensitive(true);
        if panel.epoch.get() == epoch {
            panel.net.set(Some(result));
        }
        panel.render();
    });
}

fn row(title: &str) -> (gtk4::Box, gtk4::Label) {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let name = gtk4::Label::new(Some(title));
    name.set_hexpand(true);
    name.set_xalign(0.0);
    let state = gtk4::Label::new(Some("—"));
    state.add_css_class("heading");
    row.append(&name);
    row.append(&state);
    (row, state)
}

/// Wi-Fi button in the header bar that opens the connection check.
pub fn button(ipc: &Arc<Ipc>) -> gtk4::MenuButton {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_width_request(360);
    let title = gtk4::Label::new(Some("Проверка соединения"));
    title.add_css_class("title-4");
    title.set_xalign(0.0);
    content.append(&title);
    let (internet_row, internet) = row("Ваш интернет");
    let (video_row, video) = row("Видео с сайта");
    content.append(&internet_row);
    content.append(&video_row);
    let detail = gtk4::Label::new(None);
    detail.add_css_class("dim-label");
    detail.add_css_class("caption");
    detail.set_wrap(true);
    detail.set_xalign(0.0);
    content.append(&detail);
    content.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
    let verdict_title = gtk4::Label::new(None);
    verdict_title.add_css_class("heading");
    verdict_title.set_xalign(0.0);
    verdict_title.set_wrap(true);
    let verdict_text = gtk4::Label::new(None);
    verdict_text.set_wrap(true);
    verdict_text.set_max_width_chars(44);
    verdict_text.set_xalign(0.0);
    content.append(&verdict_title);
    content.append(&verdict_text);
    let recheck = gtk4::Button::with_label("Проверить снова");
    recheck.set_halign(gtk4::Align::Start);
    content.append(&recheck);

    let panel = Rc::new(Panel {
        internet,
        video,
        detail,
        verdict_title,
        verdict_text,
        recheck: recheck.clone(),
        net: Cell::new(None),
        measuring: Cell::new(false),
        seg: RefCell::new(None),
        epoch: Cell::new(0),
    });

    let popover = gtk4::Popover::builder().child(&content).build();
    let poll: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
    popover.connect_show(glib::clone!(
        #[strong]
        panel,
        #[strong]
        ipc,
        #[strong]
        poll,
        move |_| {
            panel.epoch.set(panel.epoch.get() + 1);
            panel.net.set(None);
            let refresh = glib::clone!(
                #[strong]
                panel,
                #[strong]
                ipc,
                move || {
                    *panel.seg.borrow_mut() = fresh(ipc.segment_stats(), now_ms());
                    panel.render();
                }
            );
            refresh();
            if let Some(old) = poll.borrow_mut().replace(glib::timeout_add_local(
                Duration::from_secs(1),
                move || {
                    refresh();
                    glib::ControlFlow::Continue
                },
            )) {
                old.remove();
            }
            measure(&panel, &ipc);
        }
    ));
    popover.connect_hide(glib::clone!(
        #[strong]
        panel,
        move |_| {
            panel.epoch.set(panel.epoch.get() + 1);
            if let Some(source) = poll.borrow_mut().take() {
                source.remove();
            }
        }
    ));
    recheck.connect_clicked(glib::clone!(
        #[strong]
        panel,
        #[strong]
        ipc,
        move |_| {
            panel.net.set(None);
            measure(&panel, &ipc);
        }
    ));

    gtk4::MenuButton::builder()
        .icon_name("network-wireless-symbolic")
        .tooltip_text("Проверить соединение: интернет или сайт")
        .popover(&popover)
        .build()
}
