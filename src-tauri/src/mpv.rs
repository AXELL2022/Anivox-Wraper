use crate::discord::{DiscordState, ShikimoriInfoState};
use crate::ensure_local_window;
use crate::player_settings::{cached_player_settings, player_logs_path};
use anivox_core::mpv_session::{self as session, MpvHost, MpvLaunchRequest};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Manager;

pub(crate) use anivox_core::mpv::*;
pub(crate) use anivox_core::mpv_session::{MpvLaunchInfo, MpvState};

pub(crate) fn history_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("playback-history.json"))
}

/// Tauri side of the shared MPV session logic in `anivox-core`.
#[derive(Clone)]
struct TauriHost(tauri::AppHandle);

impl MpvHost for TauriHost {
    fn mpv(&self) -> &MpvState {
        self.0.state::<MpvState>().inner()
    }
    fn discord(&self) -> &Mutex<Option<discord_rich_presence::DiscordIpcClient>> {
        &self.0.state::<DiscordState>().inner().0
    }
    fn shikimori(&self) -> &anivox_core::shikimori::ShikimoriCache {
        &self.0.state::<ShikimoriInfoState>().inner().0
    }
    fn user_agent(&self) -> &'static str {
        crate::discord::USER_AGENT
    }
    fn site_origin(&self) -> String {
        crate::site_hosts::site_origin_for(
            self.0
                .get_webview("content")
                .and_then(|view| view.url().ok())
                .as_ref(),
        )
    }
    fn player_settings(&self) -> Result<crate::player_settings::PlayerSettings, String> {
        cached_player_settings(&self.0)
    }
    fn logs_dir(&self) -> Result<PathBuf, String> {
        player_logs_path(&self.0)
    }
    fn history_path(&self) -> Result<PathBuf, String> {
        history_path(&self.0)
    }
    fn report_status(&self, text: &str, color: &str, reset_after_ms: u64, detail: Option<String>) {
        let _ = crate::report_player_status(
            self.0.clone(),
            text.into(),
            color.into(),
            reset_after_ms,
            detail,
        );
    }
}

pub(crate) fn ensure_content_access(webview: &tauri::Webview) -> Result<(), String> {
    if webview.label() == "content" {
        Ok(())
    } else {
        Err("Доступно только в плеере Anivox".into())
    }
}

#[tauri::command]
pub fn get_mpv_state(
    app: tauri::AppHandle,
    webview: tauri::Webview,
) -> Result<Option<MpvSession>, String> {
    ensure_content_access(&webview)?;
    let value = app
        .state::<MpvState>()
        .current
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    Ok(value)
}

#[tauri::command]
pub async fn get_playback_resume(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    key: String,
) -> Result<Option<ResumePoint>, String> {
    ensure_content_access(&webview)?;
    tauri::async_runtime::spawn_blocking(move || session::playback_resume(&TauriHost(app), &key))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn clear_playback_history(
    app: tauri::AppHandle,
    webview: tauri::Webview,
) -> Result<(), String> {
    ensure_local_window(&webview, "settings")?;
    tauri::async_runtime::spawn_blocking(move || session::clear_playback_history(&TauriHost(app)))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_in_mpv(
    app: tauri::AppHandle,
    url: String,
    title: Option<String>,
    start_time: Option<f64>,
    sub_url: Option<String>,
    token: Option<String>,
    media: Option<PlaybackMedia>,
) -> Result<MpvLaunchInfo, String> {
    session::open_in_mpv(
        TauriHost(app),
        MpvLaunchRequest {
            url,
            title,
            start_time,
            sub_url,
            token,
            media,
        },
    )
    .await
}
