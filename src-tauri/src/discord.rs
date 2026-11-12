use discord_rich_presence::DiscordIpcClient;
use std::sync::Mutex;
use tauri::Manager;

#[cfg(test)]
pub(crate) use crate::discord_presence::{build_discord_activity, send_discord_activity};
pub(crate) use crate::discord_presence::{update_discord_presence, DiscordPresence};
#[cfg(test)]
pub(crate) use anivox_core::shikimori::ShikimoriData;
#[cfg(test)]
pub(crate) use anivox_core::shikimori::{
    add_episode_total, shikimori_info_episodes, shikimori_info_label, strip_episode,
};
pub(crate) use anivox_core::shikimori::{presence_title_id, site_title_id};

// State to hold the Discord IPC client
pub(crate) struct DiscordState(pub Mutex<Option<DiscordIpcClient>>);
/// Rating line fetched from the public Shikimori API, keyed by anime id.
#[derive(Default)]
pub(crate) struct ShikimoriInfoState(pub(crate) anivox_core::shikimori::ShikimoriCache);

pub(crate) const USER_AGENT: &str = concat!("Anivox/", env!("CARGO_PKG_VERSION"));

#[cfg(test)]
pub(crate) async fn fetch_shikimori_data(id: u64) -> Result<ShikimoriData, String> {
    anivox_core::shikimori::fetch_shikimori_data(id, USER_AGENT).await
}

/// Fills the cache for a title; failures are remembered so the API is not hammered.
pub(crate) async fn ensure_shikimori_info(app: &tauri::AppHandle, id: u64) {
    app.state::<ShikimoriInfoState>()
        .0
        .ensure(id, USER_AGENT)
        .await;
}

/// Puts the cached Shikimori line before the status ("★ 9.05 · TV · 2019 · Серия 5 · ...").
pub(crate) fn decorate_presence(app: &tauri::AppHandle, presence: &mut DiscordPresence) {
    app.state::<ShikimoriInfoState>().0.decorate(presence);
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
