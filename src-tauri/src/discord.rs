//! Shared Discord Rich Presence payload validation, construction, and reconnect behavior.

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

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
    if let Some(url) = discord_public_url(presence.anime_url.as_deref(), Some("anivox.fun")) {
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
