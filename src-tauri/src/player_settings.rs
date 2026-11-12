use crate::ensure_local_window;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

pub(crate) use anivox_core::player_settings::{
    load_player_settings, save_player_settings, validate_mpv_path, PlayerSettings,
};

#[derive(Default)]
/// Настройки плеера живут в памяти: диск читается один раз, запись обновляет кеш.
pub(crate) struct PlayerSettingsState(pub(crate) Mutex<Option<PlayerSettings>>);

pub(crate) fn cached_player_settings(app: &tauri::AppHandle) -> Result<PlayerSettings, String> {
    let state = app.state::<PlayerSettingsState>();
    let mut cache = state.0.lock().map_err(|err| err.to_string())?;
    if let Some(settings) = cache.as_ref() {
        return Ok(settings.clone());
    }
    let settings = load_player_settings(&player_settings_path(app)?)?;
    *cache = Some(settings.clone());
    Ok(settings)
}

pub(crate) fn player_settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|err| err.to_string())?
        .join("settings.json"))
}

pub(crate) fn player_logs_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|err| err.to_string())?
        .join("logs"))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlayerSettingsInfo {
    pub(crate) mpv_path: Option<String>,
    pub(crate) log_directory: String,
    pub(crate) mirror_embedded: bool,
}

pub(crate) fn player_settings_info(
    app: &tauri::AppHandle,
    settings: PlayerSettings,
) -> Result<PlayerSettingsInfo, String> {
    Ok(PlayerSettingsInfo {
        mpv_path: settings.mpv_path,
        mirror_embedded: settings.mirror_embedded,
        log_directory: player_logs_path(app)?.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn get_player_settings(
    app: tauri::AppHandle,
    webview: tauri::Webview,
) -> Result<PlayerSettingsInfo, String> {
    ensure_local_window(&webview, "settings")?;
    player_settings_info(&app, cached_player_settings(&app)?)
}

#[tauri::command]
pub async fn save_player_settings_command(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    mpv_path: Option<String>,
    mirror_embedded: Option<bool>,
) -> Result<PlayerSettingsInfo, String> {
    ensure_local_window(&webview, "settings")?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<PlayerSettingsState>();
        let mut cache = state.0.lock().map_err(|err| err.to_string())?;
        let mirror_embedded = match (mirror_embedded, cache.as_ref()) {
            (Some(value), _) => value,
            (None, Some(current)) => current.mirror_embedded,
            (None, None) => load_player_settings(&player_settings_path(&app)?)?.mirror_embedded,
        };
        let settings = PlayerSettings {
            mpv_path: validate_mpv_path(mpv_path)?,
            mirror_embedded,
        };
        save_player_settings(&player_settings_path(&app)?, &settings)?;
        *cache = Some(settings.clone());
        player_settings_info(&app, settings)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn pick_mpv_executable(
    app: tauri::AppHandle,
    webview: tauri::Webview,
) -> Result<Option<String>, String> {
    ensure_local_window(&webview, "settings")?;
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = app
            .dialog()
            .file()
            .set_title("Выберите MPV")
            .set_parent(&webview.window());
        #[cfg(target_os = "windows")]
        let dialog = dialog.add_filter("Исполняемый файл MPV", &["exe"]);
        dialog
            .blocking_pick_file()
            .map(|file| {
                file.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|err| err.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|err| err.to_string())?
}
