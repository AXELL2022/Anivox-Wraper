//! Connection check popover: is it the user's internet or the video server?
//!
//! `segment-monitor.js` passively reports how video segments download
//! (`report_segment_stats`); the popover compares that with a download from
//! speed.cloudflare.com. The verdict logic mirrors `ui/connection.js`.

use crate::ipc::Ipc;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

const SPEED_URL: &str = "https://speed.cloudflare.com/__down?bytes=8000000";
const SPEED_LIMIT: Duration = Duration::from_secs(6);
/// Site data older than this counts as missing.
const STALE_MS: f64 = 4000.0;
/// Below this speed a value is shown in red.
pub const GOOD_MBPS: f64 = 10.0;
/// A segment without a single byte for longer than this is "stalled".
const STALL_MS: u64 = 5000;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SegmentStats {
    pub host: String,
    pub active: bool,
    pub inflight: u32,
    pub stalled_ms: u64,
    pub has_speed: bool,
    pub mbps: f64,
    pub slow: u32,
    pub failed: u32,
    pub samples: u32,
    pub max_ms: u64,
    pub updated_at: f64,
}

impl SegmentStats {
    /// Values come from a web page: bound the host and drop impossible speeds.
    pub fn sanitized(mut self) -> Self {
        self.host = self.host.chars().take(120).collect();
        if !self.mbps.is_finite() || self.mbps < 0.0 {
            self.mbps = 0.0;
        }
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetResult {
    pub ok: bool,
    pub mbps: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Good,
    Bad,
    Warn,
    Pending,
}

#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub internet: Option<(Kind, String)>,
    pub video: (Kind, String),
    pub detail: Option<String>,
    pub verdict: (Kind, String, String),
}

#[derive(Clone, Copy, PartialEq)]
enum VideoState {
    None,
    Stalled,
    Failing,
    Idle,
    Slow,
    Ok,
}

fn fmt(mbps: f64) -> String {
    if mbps >= 10.0 {
        format!("{mbps:.0}")
    } else {
        format!("{mbps:.1}")
    }
}

fn video_state(seg: Option<&SegmentStats>) -> VideoState {
    let Some(seg) = seg.filter(|seg| seg.active) else {
        return VideoState::None;
    };
    if seg.inflight > 0 && seg.stalled_ms >= STALL_MS {
        VideoState::Stalled
    } else if seg.failed > 0
        && seg.failed as f64 >= seg.samples.max(1) as f64 / 2.0
        && !seg.has_speed
    {
        VideoState::Failing
    } else if !seg.has_speed {
        VideoState::Idle
    } else if seg.mbps < GOOD_MBPS {
        VideoState::Slow
    } else {
        VideoState::Ok
    }
}

/// Fresh statistics only: a closed player or a page that stopped reporting is "no video".
pub fn fresh(stats: Option<SegmentStats>, now_ms: f64) -> Option<SegmentStats> {
    stats.filter(|stats| (now_ms - stats.updated_at).abs() < STALE_MS)
}

pub fn assess(net: Option<NetResult>, measuring: bool, seg: Option<&SegmentStats>) -> View {
    let state = video_state(seg);
    let seg_mbps = seg.map_or(0.0, |seg| seg.mbps);
    let host = seg.map(|seg| seg.host.as_str()).unwrap_or("");
    let playing = matches!(state, VideoState::Ok | VideoState::Slow);
    // The internet cannot be slower than video that is already arriving.
    let net_mbps = net
        .map_or(0.0, |net| net.mbps)
        .max(if playing { seg_mbps } else { 0.0 });
    let net_known = net.is_some() || state == VideoState::Ok;
    let net_failed = net.is_some_and(|net| !net.ok);

    let internet = if measuring && !net_known {
        Some((Kind::Pending, "Измеряем…".to_owned()))
    } else if net_failed && net_mbps == 0.0 {
        Some((Kind::Bad, "Нет связи".to_owned()))
    } else if net_known {
        let kind = if net_mbps < GOOD_MBPS {
            Kind::Bad
        } else {
            Kind::Good
        };
        let suffix = if measuring {
            " · измеряем…"
        } else {
            ""
        };
        Some((kind, format!("{} Мбит/с{suffix}", fmt(net_mbps))))
    } else {
        None
    };

    let video = match state {
        VideoState::None => (Kind::Pending, "Видео не идёт".to_owned()),
        VideoState::Stalled => (
            Kind::Bad,
            format!(
                "Завис {} с",
                (seg.map_or(0, |s| s.stalled_ms) as f64 / 1000.0).round()
            ),
        ),
        VideoState::Failing => (Kind::Bad, "Ошибки загрузки".to_owned()),
        VideoState::Idle => (Kind::Pending, "Идёт буферизация…".to_owned()),
        VideoState::Slow => (Kind::Bad, format!("{} Мбит/с", fmt(seg_mbps))),
        VideoState::Ok => (Kind::Good, format!("{} Мбит/с", fmt(seg_mbps))),
    };
    let detail = seg.filter(|_| state != VideoState::None).map(|seg| {
        let mut parts = Vec::new();
        if !seg.host.is_empty() {
            parts.push(format!("Сервер: {}", seg.host));
        }
        parts.push(format!("в загрузке: {}", seg.inflight));
        if seg.slow > 0 {
            parts.push(format!("медленных (>8 с): {}", seg.slow));
        }
        if seg.failed > 0 {
            parts.push(format!("ошибок: {}", seg.failed));
        }
        parts.join(" · ")
    });

    let verdict = |kind: Kind, title: &str, text: String| (kind, title.to_owned(), text);
    let verdict = if matches!(state, VideoState::Stalled | VideoState::Failing) {
        let place = if host.is_empty() {
            String::new()
        } else {
            format!(" ({host})")
        };
        if net_known && net_mbps >= GOOD_MBPS && !net_failed {
            let what = if state == VideoState::Stalled {
                format!(
                    "не приходит уже {} с",
                    (seg.map_or(0, |s| s.stalled_ms) as f64 / 1000.0).round()
                )
            } else {
                "не загружается".to_owned()
            };
            verdict(
                Kind::Warn,
                "Тормозит видео-сервер, а не ваш интернет",
                format!(
                    "Интернет даёт {} Мбит/с, а кусок видео{place} {what}. Попробуйте позже, другую озвучку или VPN.",
                    fmt(net_mbps)
                ),
            )
        } else if measuring || !net_known {
            verdict(
                Kind::Pending,
                "Видео не загружается",
                "Измеряем ваш интернет, чтобы понять, чья это проблема…".to_owned(),
            )
        } else if net_failed {
            verdict(
                Kind::Bad,
                "Проблема с вашим интернетом",
                "Не удалось скачать даже тестовый файл. Проверьте соединение, VPN и прокси."
                    .to_owned(),
            )
        } else {
            verdict(
                Kind::Bad,
                "Проблема с вашим интернетом",
                format!(
                    "Ваш канал даёт всего {} Мбит/с — видео не успевает загружаться.",
                    fmt(net_mbps)
                ),
            )
        }
    } else if measuring && !net_known {
        verdict(
            Kind::Pending,
            "Проверяем…",
            "Измеряем скорость интернета.".to_owned(),
        )
    } else if net_failed && net_mbps == 0.0 {
        verdict(
            Kind::Bad,
            "Проблема с вашим интернетом",
            "Не удалось скачать тестовый файл с быстрого сервера. Проверьте соединение, VPN и прокси."
                .to_owned(),
        )
    } else {
        match state {
            VideoState::None if net_mbps < GOOD_MBPS => verdict(
                Kind::Bad,
                "Ваш интернет медленный",
                format!(
                    "Скорость {} Мбит/с. Включите серию — панель покажет состояние видео в реальном времени.",
                    fmt(net_mbps)
                ),
            ),
            VideoState::None => verdict(
                Kind::Pending,
                "Интернет в порядке",
                format!(
                    "Скорость {} Мбит/с. Включите серию — панель покажет состояние видео в реальном времени.",
                    fmt(net_mbps)
                ),
            ),
            VideoState::Idle => verdict(
                Kind::Pending,
                "Видео загружается…",
                "Данные ещё накапливаются, подождите пару секунд.".to_owned(),
            ),
            VideoState::Slow if net_mbps >= GOOD_MBPS && seg_mbps < net_mbps * 0.4 => verdict(
                Kind::Warn,
                "Тормозит видео-сервер, а не ваш интернет",
                format!(
                    "Интернет даёт {} Мбит/с, а видео приходит лишь {} Мбит/с. Попробуйте позже, другую озвучку или VPN.",
                    fmt(net_mbps),
                    fmt(seg_mbps)
                ),
            ),
            VideoState::Slow => verdict(
                Kind::Bad,
                "Проблема с вашим интернетом",
                format!(
                    "Канал даёт {} Мбит/с, видео приходит {} Мбит/с — для плавного просмотра мало.",
                    fmt(net_mbps),
                    fmt(seg_mbps)
                ),
            ),
            _ => verdict(
                Kind::Good,
                "Всё в порядке",
                format!(
                    "Видео приходит со скоростью {} Мбит/с — воспроизведение должно идти плавно.",
                    fmt(seg_mbps)
                ),
            ),
        }
    };
    View {
        internet,
        video,
        detail,
        verdict,
    }
}

/// Downloads a test file for up to six seconds and reports the speed.
pub async fn measure_internet() -> NetResult {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let started = std::time::Instant::now();
    let mut bytes = 0usize;
    let download = async {
        let client = reqwest::Client::builder()
            .user_agent(concat!("Anivox/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let mut response = client
            .get(format!("{SPEED_URL}&t={stamp}"))
            .send()
            .await?
            .error_for_status()?;
        while let Some(chunk) = response.chunk().await? {
            bytes += chunk.len();
        }
        Ok::<(), reqwest::Error>(())
    };
    // A partial download still measures the speed; only "nothing arrived" is a failure.
    let _ = tokio::time::timeout(SPEED_LIMIT, download).await;
    if bytes == 0 {
        return NetResult {
            ok: false,
            mbps: 0.0,
        };
    }
    let seconds = started.elapsed().as_secs_f64().max(0.001);
    NetResult {
        ok: true,
        mbps: bytes as f64 * 8.0 / seconds / 1e6,
    }
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1000.0
}

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
        if let Some((kind, text)) = view.internet {
            self.internet.set_text(&text);
            paint(&self.internet, kind);
        }
        self.video.set_text(&view.video.1);
        paint(&self.video, view.video.0);
        self.detail.set_visible(view.detail.is_some());
        self.detail.set_text(view.detail.as_deref().unwrap_or(""));
        let (kind, title, text) = view.verdict;
        self.verdict_title.set_text(&title);
        paint(&self.verdict_title, kind);
        self.verdict_text.set_text(&text);
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
        let result = gio::spawn_blocking(move || ipc.block_on(measure_internet()))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(mbps: f64) -> SegmentStats {
        SegmentStats {
            host: "cdn.test".into(),
            active: true,
            inflight: 1,
            has_speed: true,
            mbps,
            samples: 4,
            updated_at: 1000.0,
            ..SegmentStats::default()
        }
    }

    #[test]
    fn stale_or_inactive_statistics_mean_no_video() {
        assert!(fresh(Some(seg(20.0)), 6000.0).is_none());
        assert!(fresh(Some(seg(20.0)), 2000.0).is_some());
        let view = assess(None, true, None);
        assert_eq!(view.video.1, "Видео не идёт");
        assert_eq!(view.verdict.1, "Проверяем…");
        assert_eq!(view.internet.unwrap().1, "Измеряем…");
    }

    #[test]
    fn slow_server_on_fast_internet_blames_the_server() {
        let net = NetResult {
            ok: true,
            mbps: 100.0,
        };
        let view = assess(Some(net), false, Some(&seg(3.0)));
        assert_eq!(view.verdict.0, Kind::Warn);
        assert!(view.verdict.2.contains("100"), "{}", view.verdict.2);
        let stalled = SegmentStats {
            stalled_ms: 7000,
            ..seg(20.0)
        };
        let view = assess(Some(net), false, Some(&stalled));
        assert_eq!(view.video, (Kind::Bad, "Завис 7 с".into()));
        assert!(view.verdict.2.contains("(cdn.test) не приходит уже 7 с"));
    }

    #[test]
    fn slow_internet_is_reported_as_the_users_problem() {
        let net = NetResult {
            ok: true,
            mbps: 4.0,
        };
        let view = assess(Some(net), false, Some(&seg(3.5)));
        assert_eq!(view.verdict.0, Kind::Bad);
        assert_eq!(view.verdict.1, "Проблема с вашим интернетом");
        let offline = NetResult {
            ok: false,
            mbps: 0.0,
        };
        let view = assess(Some(offline), false, None);
        assert_eq!(view.internet.unwrap(), (Kind::Bad, "Нет связи".into()));
    }

    #[test]
    fn fast_video_is_fine_and_proves_the_internet() {
        let view = assess(None, true, Some(&seg(25.0)));
        assert_eq!(view.verdict.0, Kind::Good);
        assert_eq!(
            view.internet.unwrap(),
            (Kind::Good, "25 Мбит/с · измеряем…".into())
        );
        assert_eq!(
            view.detail.as_deref(),
            Some("Сервер: cdn.test · в загрузке: 1")
        );
    }

    #[test]
    fn page_values_are_bounded() {
        let stats = SegmentStats {
            host: "x".repeat(500),
            mbps: f64::NAN,
            ..SegmentStats::default()
        }
        .sanitized();
        assert_eq!(stats.host.len(), 120);
        assert_eq!(stats.mbps, 0.0);
    }
}

#[cfg(test)]
mod network_tests {
    /// Needs the internet: `cargo test -- --ignored network`.
    #[test]
    #[ignore = "needs the internet"]
    fn network_speed_test_and_shikimori_work_over_https() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let net = runtime.block_on(super::measure_internet());
        eprintln!("speed: {net:?}");
        assert!(net.ok && net.mbps > 0.0);
        let data = runtime
            .block_on(anivox_core::shikimori::fetch_shikimori_data(
                5114,
                "Anivox-test",
            ))
            .unwrap();
        eprintln!("shikimori: {:?}", data.label);
        assert!(data.label.is_some());
    }
}
