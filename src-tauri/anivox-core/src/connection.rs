//! Connection check: is it the user's internet or the video server?
//!
//! The verdict logic (ported from the former Tauri `ui/connection.js`): `segment-monitor.js` passively
//! reports how video segments download, the native panel compares that with a
//! download from speed.cloudflare.com. Shells only draw the [`View`].

use crate::segments::SegmentStats;
use serde::{Deserialize, Serialize};

#[cfg(feature = "net")]
const SPEED_URL: &str = "https://speed.cloudflare.com/__down?bytes=8000000";
#[cfg(feature = "net")]
const SPEED_LIMIT: std::time::Duration = std::time::Duration::from_secs(6);
/// Site data older than this counts as missing.
const STALE_MS: f64 = 4000.0;
/// Below this speed a value is shown in red.
pub const GOOD_MBPS: f64 = 10.0;
/// A segment without a single byte for longer than this is "stalled".
const STALL_MS: u64 = 5000;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetResult {
    pub ok: bool,
    pub mbps: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Good,
    Bad,
    Warn,
    Pending,
}

/// A line of the panel: color kind and text.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub kind: Kind,
    pub title: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub internet: Option<Line>,
    pub video: Line,
    pub detail: Option<String>,
    pub verdict: Verdict,
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

fn line(kind: Kind, text: impl Into<String>) -> Line {
    Line {
        kind,
        text: text.into(),
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
        Some(line(Kind::Pending, "Измеряем…"))
    } else if net_failed && net_mbps == 0.0 {
        Some(line(Kind::Bad, "Нет связи"))
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
        Some(line(kind, format!("{} Мбит/с{suffix}", fmt(net_mbps))))
    } else {
        None
    };

    let stalled_s = (seg.map_or(0, |s| s.stalled_ms) as f64 / 1000.0).round();
    let video = match state {
        VideoState::None => line(Kind::Pending, "Видео не идёт"),
        VideoState::Stalled => line(Kind::Bad, format!("Завис {stalled_s} с")),
        VideoState::Failing => line(Kind::Bad, "Ошибки загрузки"),
        VideoState::Idle => line(Kind::Pending, "Идёт буферизация…"),
        VideoState::Slow => line(Kind::Bad, format!("{} Мбит/с", fmt(seg_mbps))),
        VideoState::Ok => line(Kind::Good, format!("{} Мбит/с", fmt(seg_mbps))),
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

    let verdict = |kind: Kind, title: &str, text: String| Verdict {
        kind,
        title: title.to_owned(),
        text,
    };
    let verdict = if matches!(state, VideoState::Stalled | VideoState::Failing) {
        let place = if host.is_empty() {
            String::new()
        } else {
            format!(" ({host})")
        };
        if net_known && net_mbps >= GOOD_MBPS && !net_failed {
            let what = if state == VideoState::Stalled {
                format!("не приходит уже {stalled_s} с")
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
#[cfg(feature = "net")]
pub async fn measure_internet(user_agent: &str) -> NetResult {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let started = std::time::Instant::now();
    let mut bytes = 0usize;
    let download = async {
        let client = reqwest::Client::builder().user_agent(user_agent).build()?;
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

pub fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1000.0
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
        assert_eq!(view.video.text, "Видео не идёт");
        assert_eq!(view.verdict.title, "Проверяем…");
        assert_eq!(view.internet.unwrap().text, "Измеряем…");
    }

    #[test]
    fn slow_server_on_fast_internet_blames_the_server() {
        let net = NetResult {
            ok: true,
            mbps: 100.0,
        };
        let view = assess(Some(net), false, Some(&seg(3.0)));
        assert_eq!(view.verdict.kind, Kind::Warn);
        assert!(view.verdict.text.contains("100"), "{}", view.verdict.text);
        let stalled = SegmentStats {
            stalled_ms: 7000,
            ..seg(20.0)
        };
        let view = assess(Some(net), false, Some(&stalled));
        assert_eq!(view.video, line(Kind::Bad, "Завис 7 с"));
        assert!(view.verdict.text.contains("(cdn.test) не приходит уже 7 с"));
    }

    #[test]
    fn slow_internet_is_reported_as_the_users_problem() {
        let net = NetResult {
            ok: true,
            mbps: 4.0,
        };
        let view = assess(Some(net), false, Some(&seg(3.5)));
        assert_eq!(view.verdict.kind, Kind::Bad);
        assert_eq!(view.verdict.title, "Проблема с вашим интернетом");
        let offline = NetResult {
            ok: false,
            mbps: 0.0,
        };
        let view = assess(Some(offline), false, None);
        assert_eq!(view.internet.unwrap(), line(Kind::Bad, "Нет связи"));
    }

    #[test]
    fn fast_video_is_fine_and_proves_the_internet() {
        let view = assess(None, true, Some(&seg(25.0)));
        assert_eq!(view.verdict.kind, Kind::Good);
        assert_eq!(
            view.internet.unwrap(),
            line(Kind::Good, "25 Мбит/с · измеряем…")
        );
        assert_eq!(
            view.detail.as_deref(),
            Some("Сервер: cdn.test · в загрузке: 1")
        );
    }

    #[test]
    fn view_serializes_for_the_shell() {
        let value = serde_json::to_value(assess(None, false, None)).unwrap();
        assert_eq!(value["video"]["kind"], "pending");
        assert!(value["verdict"]["title"].is_string());
    }
}

#[cfg(all(test, feature = "net"))]
mod network_tests {
    /// Needs the internet: `cargo test -- --ignored network`.
    #[test]
    #[ignore = "needs the internet"]
    fn network_speed_test_and_shikimori_work_over_https() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let net = runtime.block_on(super::measure_internet("Anivox-test"));
        eprintln!("speed: {net:?}");
        assert!(net.ok && net.mbps > 0.0);
        let data = runtime
            .block_on(crate::shikimori::fetch_shikimori_data(5114, "Anivox-test"))
            .unwrap();
        eprintln!("shikimori: {:?}", data.label);
        assert!(data.label.is_some());
    }
}
