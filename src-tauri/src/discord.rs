use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::sync::Mutex;
use tauri::Manager;

// State to hold the Discord IPC client
pub(crate) struct DiscordState(pub Mutex<Option<DiscordIpcClient>>);

pub(crate) fn send_discord_activity<C>(
    client: &mut Option<C>,
    connect: impl FnOnce() -> Result<C, String>,
    send: impl FnOnce(&mut C) -> Result<(), String>,
) -> Result<(), String> {
    if client.is_none() {
        *client = Some(connect()?);
    }
    let result = send(client.as_mut().unwrap());
    if result.is_err() {
        // Discard the broken transport; the next heartbeat establishes a new one.
        *client = None;
    }
    result
}

#[derive(Clone, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct DiscordPresence {
    pub(crate) details: String,
    pub(crate) state_text: String,
    pub(crate) poster_url: Option<String>,
    pub(crate) anime_url: Option<String>,
    pub(crate) shikimori_url: Option<String>,
    pub(crate) end_timestamp: Option<i64>,
}

fn discord_public_url<'a>(value: Option<&'a str>, host: Option<&str>) -> Option<&'a str> {
    let value = value?;
    let url = tauri::Url::parse(value).ok()?;
    if value.len() > 512
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || host.is_some_and(|host| url.host_str() != Some(host))
    {
        return None;
    }
    Some(value)
}

pub(crate) fn build_discord_activity(presence: &DiscordPresence) -> activity::Activity<'_> {
    let mut activity = activity::Activity::new().activity_type(activity::ActivityType::Watching);
    if !presence.details.is_empty() {
        activity = activity.details(&presence.details);
    }
    if !presence.state_text.is_empty() {
        activity = activity.state(&presence.state_text);
    }
    let poster =
        discord_public_url(presence.poster_url.as_deref(), None).filter(|url| url.len() <= 256);
    let mut assets = activity::Assets::new()
        .large_image(poster.unwrap_or("logo"))
        .large_text(if presence.details.is_empty() {
            "Anivox"
        } else {
            &presence.details
        });
    if poster.is_some() {
        assets = assets.small_image("logo").small_text("Anivox");
    }
    activity = activity.assets(assets);
    let mut buttons = Vec::new();
    if let Some(url) = discord_public_url(presence.anime_url.as_deref(), None).filter(|url| {
        tauri::Url::parse(url).is_ok_and(|url| crate::site_hosts::is_site_origin(&url))
    }) {
        buttons.push(activity::Button::new("Открыть аниме", url));
    }
    if let Some(url) = discord_public_url(presence.shikimori_url.as_deref(), Some("shikimori.io")) {
        buttons.push(activity::Button::new("Shikimori", url));
    }
    if !buttons.is_empty() {
        activity = activity.buttons(buttons);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    if let Some(end) = presence
        .end_timestamp
        .filter(|end| *end > now && *end <= now + 7 * 86400)
    {
        activity = activity.timestamps(activity::Timestamps::new().end(end));
    }
    activity
}

pub(crate) fn update_discord_presence(
    client: &mut Option<DiscordIpcClient>,
    presence: &DiscordPresence,
) -> Result<(), String> {
    send_discord_activity(
        client,
        || {
            let mut client =
                DiscordIpcClient::new("1504862803335315609").map_err(|e| e.to_string())?;
            client.connect().map_err(|e| e.to_string())?;
            Ok(client)
        },
        |client| {
            client
                .set_activity(build_discord_activity(presence))
                .map_err(|e| e.to_string())
        },
    )
}

const SHIKIMORI_INFO_RETRY: std::time::Duration = std::time::Duration::from_secs(300);

#[derive(Clone, Default)]
pub(crate) struct ShikimoriData {
    pub(crate) label: Option<String>,
    /// Episodes in this season (Shikimori ids are per season); `None` when unknown.
    pub(crate) episodes: Option<u32>,
    /// A single-part film: "Серия 1" would only add noise.
    pub(crate) movie: bool,
}

enum ShikimoriInfo {
    Ready(ShikimoriData),
    Failed(std::time::Instant),
}

/// Rating line fetched from the public Shikimori API, keyed by anime id.
#[derive(Default)]
pub(crate) struct ShikimoriInfoState(Mutex<std::collections::HashMap<u64, ShikimoriInfo>>);

/// Anivox (`/anime/38524-slug`) and Shikimori (`/animes/38524-slug`) share one id.
pub(crate) fn title_id(url: &tauri::Url, host: &str, section: &str) -> Option<u64> {
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
pub(crate) fn site_title_id(url: &tauri::Url) -> Option<u64> {
    if !crate::site_hosts::is_site_origin(url) {
        return None;
    }
    title_id(url, url.host_str()?, "anime")
}

pub(crate) fn presence_title_id(presence: &DiscordPresence) -> Option<u64> {
    let by_shikimori = presence
        .shikimori_url
        .as_deref()
        .and_then(|url| tauri::Url::parse(url).ok())
        .and_then(|url| title_id(&url, "shikimori.io", "animes"));
    // Anivox pages share the Shikimori id, which covers titles without `shikimori_id`.
    by_shikimori.or_else(|| {
        let url = tauri::Url::parse(presence.anime_url.as_deref()?).ok()?;
        site_title_id(&url)
    })
}

pub(crate) fn shikimori_info_label(info: &serde_json::Value) -> Option<String> {
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

pub(crate) fn shikimori_info_episodes(info: &serde_json::Value) -> Option<u32> {
    info.get("episodes")?
        .as_u64()
        .filter(|n| (1..=9999).contains(n))
        .map(|n| n as u32)
}

/// "Серия 8 · ..." -> "Серия 8/10 · ..."; other texts and out-of-range numbers are kept.
pub(crate) fn add_episode_total(state: &str, total: u32) -> String {
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
pub(crate) fn strip_episode(state: &str) -> String {
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

pub(crate) async fn fetch_shikimori_data(id: u64) -> Result<ShikimoriData, String> {
    // reqwest is built without a bundled TLS provider (shared with the updater).
    // An error only means a provider is already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .user_agent(concat!("Anivox/", env!("CARGO_PKG_VERSION")))
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

/// Fills the cache for a title; failures are remembered so the API is not hammered.
pub(crate) async fn ensure_shikimori_info(app: &tauri::AppHandle, id: u64) {
    {
        let cache = app.state::<ShikimoriInfoState>();
        let Ok(cache) = cache.0.lock() else { return };
        match cache.get(&id) {
            Some(ShikimoriInfo::Ready(_)) => return,
            Some(ShikimoriInfo::Failed(at)) if at.elapsed() < SHIKIMORI_INFO_RETRY => return,
            _ => {}
        }
    }
    let entry = match fetch_shikimori_data(id).await {
        Ok(data) => ShikimoriInfo::Ready(data),
        Err(_) => ShikimoriInfo::Failed(std::time::Instant::now()),
    };
    if let Ok(mut cache) = app.state::<ShikimoriInfoState>().0.lock() {
        if cache.len() >= 200 {
            cache.clear();
        }
        cache.insert(id, entry);
    }
}

/// Puts the cached Shikimori line before the status ("★ 9.05 · TV · 2019 · Серия 5 · ...").
pub(crate) fn decorate_presence(app: &tauri::AppHandle, presence: &mut DiscordPresence) {
    let Some(id) = presence_title_id(presence) else {
        return;
    };
    let data = match app.state::<ShikimoriInfoState>().0.lock() {
        Ok(cache) => match cache.get(&id) {
            Some(ShikimoriInfo::Ready(data)) => data.clone(),
            _ => return,
        },
        Err(_) => return,
    };
    let mut text = match data.episodes {
        _ if data.movie => strip_episode(&presence.state_text),
        Some(total) => add_episode_total(&presence.state_text, total),
        None => presence.state_text.clone(),
    };
    if let Some(label) = data.label {
        text = if text.is_empty() {
            label
        } else {
            format!("{label} · {text}")
        };
    }
    presence.state_text = text.chars().take(128).collect();
}

#[tauri::command]
pub async fn set_discord_rpc(
    app: tauri::AppHandle,
    mut presence: DiscordPresence,
) -> Result<(), String> {
    presence.details = presence.details.chars().take(128).collect();
    presence.state_text = presence.state_text.chars().take(128).collect();
    if let Some(id) = presence_title_id(&presence) {
        ensure_shikimori_info(&app, id).await;
    }
    // Discord's handshake performs blocking pipe/socket I/O without a timeout.
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<DiscordState>();
        let mut client = state.inner().0.lock().map_err(|e| e.to_string())?;
        let mpv = app.state::<crate::mpv::MpvState>();
        *mpv.embedded.lock().map_err(|e| e.to_string())? = presence.clone();
        if mpv
            .current
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .is_some_and(|s| s.playback.ready)
        {
            return Ok(());
        }
        let mut presence = presence;
        decorate_presence(&app, &mut presence);
        update_discord_presence(&mut client, &presence)
    })
    .await
    .map_err(|e| e.to_string())?
}
