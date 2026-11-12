//! Shikimori title ids, rating line and Discord status decoration.

use crate::discord_presence::DiscordPresence;
use url::Url;

#[derive(Clone, Default)]
pub struct ShikimoriData {
    pub label: Option<String>,
    /// Episodes in this season (Shikimori ids are per season); `None` when unknown.
    pub episodes: Option<u32>,
    /// A single-part film: "Серия 1" would only add noise.
    pub movie: bool,
}

/// Anivox (`/anime/38524-slug`) and Shikimori (`/animes/38524-slug`) share one id.
pub fn title_id(url: &Url, host: &str, section: &str) -> Option<u64> {
    if url.host_str() != Some(host) {
        return None;
    }
    let mut segments = url.path_segments()?;
    if segments.next()? != section {
        return None;
    }
    let segment = segments.next()?;
    let digits = segment.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 12 || segment.len() > digits && segment.as_bytes()[digits] != b'-' {
        return None;
    }
    segment[..digits].parse().ok().filter(|id| *id > 0)
}

/// Title id from an Anivox page (`/anime/<id>-slug`) on the main site or a mirror.
pub fn site_title_id(url: &Url) -> Option<u64> {
    if !crate::site_hosts::is_site_origin(url) {
        return None;
    }
    title_id(url, url.host_str()?, "anime")
}

pub fn presence_title_id(presence: &DiscordPresence) -> Option<u64> {
    let by_shikimori = presence
        .shikimori_url
        .as_deref()
        .and_then(|url| Url::parse(url).ok())
        .and_then(|url| title_id(&url, "shikimori.io", "animes"));
    // Anivox pages share the Shikimori id, which covers titles without `shikimori_id`.
    by_shikimori.or_else(|| {
        let url = Url::parse(presence.anime_url.as_deref()?).ok()?;
        site_title_id(&url)
    })
}

pub fn shikimori_info_label(info: &serde_json::Value) -> Option<String> {
    let mut parts = Vec::new();
    let score = info.get("score")?.as_str()?.parse::<f64>().ok()?;
    if score > 0.0 {
        parts.push(format!("★ {score:.2}"));
    }
    if let Some(kind) = info.get("kind").and_then(|k| k.as_str()) {
        parts.push(
            match kind {
                "tv" => "TV",
                "movie" => "Фильм",
                "ova" => "OVA",
                "ona" => "ONA",
                "special" | "tv_special" => "Спешл",
                _ => "",
            }
            .to_string(),
        );
    }
    if let Some(year) = info
        .get("aired_on")
        .and_then(|d| d.as_str())
        .and_then(|d| d.get(..4))
        .filter(|y| y.bytes().all(|c| c.is_ascii_digit()))
    {
        parts.push(year.to_string());
    }
    parts.retain(|part| !part.is_empty());
    (!parts.is_empty()).then(|| parts.join(" · "))
}

pub fn shikimori_info_episodes(info: &serde_json::Value) -> Option<u32> {
    info.get("episodes")?
        .as_u64()
        .filter(|n| (1..=9999).contains(n))
        .map(|n| n as u32)
}

/// "Серия 8 · ..." -> "Серия 8/10 · ..."; other texts and out-of-range numbers are kept.
pub fn add_episode_total(state: &str, total: u32) -> String {
    const PREFIX: &str = "Серия ";
    let Some(rest) = state.strip_prefix(PREFIX) else {
        return state.to_string();
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let number = rest[..digits].parse::<u32>().ok();
    if number.is_none_or(|n| n == 0 || n > total) || rest[digits..].starts_with('/') {
        return state.to_string();
    }
    format!("{PREFIX}{}/{total}{}", &rest[..digits], &rest[digits..])
}

/// "Серия 1 · MPV · Смотрит" -> "MPV · Смотрит"; texts without an episode are kept.
pub fn strip_episode(state: &str) -> String {
    let Some(rest) = state.strip_prefix("Серия ") else {
        return state.to_string();
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return state.to_string();
    }
    let rest = &rest[digits..];
    rest.strip_prefix(" · ").unwrap_or(rest).to_string()
}

#[cfg(feature = "net")]
pub async fn fetch_shikimori_data(id: u64, user_agent: &str) -> Result<ShikimoriData, String> {
    // reqwest is built without a bundled TLS provider (shared with the updater).
    // An error only means a provider is already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .user_agent(user_agent)
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .map_err(|e| e.to_string())?;
    let info: serde_json::Value = client
        .get(format!("https://shikimori.io/api/animes/{id}"))
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(ShikimoriData {
        label: shikimori_info_label(&info),
        episodes: shikimori_info_episodes(&info),
        movie: info.get("kind").and_then(|k| k.as_str()) == Some("movie"),
    })
}

/// Puts the Shikimori line before the status ("★ 9.05 · TV · 2019 · Серия 5 · ...").
pub fn decorate_with(data: &ShikimoriData, presence: &mut DiscordPresence) {
    let mut text = match data.episodes {
        _ if data.movie => strip_episode(&presence.state_text),
        Some(total) => add_episode_total(&presence.state_text, total),
        None => presence.state_text.clone(),
    };
    if let Some(label) = data.label.clone() {
        text = if text.is_empty() {
            label
        } else {
            format!("{label} · {text}")
        };
    }
    presence.state_text = text.chars().take(128).collect();
}

const RETRY: std::time::Duration = std::time::Duration::from_secs(300);
const CACHE_LIMIT: usize = 200;

enum Entry {
    Ready(ShikimoriData),
    Failed(std::time::Instant),
}

/// Rating lines fetched from the public Shikimori API, keyed by title id.
/// Failures are remembered for five minutes so the API is not hammered.
#[derive(Default)]
pub struct ShikimoriCache(std::sync::Mutex<std::collections::HashMap<u64, Entry>>);

impl ShikimoriCache {
    /// True when the title is neither cached nor recently failed.
    pub fn needs_fetch(&self, id: u64) -> bool {
        let Ok(cache) = self.0.lock() else {
            return false;
        };
        match cache.get(&id) {
            Some(Entry::Ready(_)) => false,
            Some(Entry::Failed(at)) if at.elapsed() < RETRY => false,
            _ => true,
        }
    }

    pub fn store(&self, id: u64, result: Result<ShikimoriData, String>) {
        if let Ok(mut cache) = self.0.lock() {
            if cache.len() >= CACHE_LIMIT {
                cache.clear();
            }
            cache.insert(
                id,
                match result {
                    Ok(data) => Entry::Ready(data),
                    Err(_) => Entry::Failed(std::time::Instant::now()),
                },
            );
        }
    }

    /// Fetches the title once unless it is cached or failed recently.
    #[cfg(feature = "net")]
    pub async fn ensure(&self, id: u64, user_agent: &str) {
        if self.needs_fetch(id) {
            self.store(id, fetch_shikimori_data(id, user_agent).await);
        }
    }

    /// Puts the cached Shikimori line before the status; no-op when unknown.
    pub fn decorate(&self, presence: &mut DiscordPresence) {
        let Some(id) = presence_title_id(presence) else {
            return;
        };
        let data = match self.0.lock() {
            Ok(cache) => match cache.get(&id) {
                Some(Entry::Ready(data)) => data.clone(),
                _ => return,
            },
            Err(_) => return,
        };
        decorate_with(&data, presence);
    }
}
