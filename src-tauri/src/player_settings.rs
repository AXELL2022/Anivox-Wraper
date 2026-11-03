use crate::ensure_local_window;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct PlayerSettings {
    pub(crate) mpv_path: Option<String>,
    pub(crate) mirror_embedded: bool,
}

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

/// On macOS the picker can return the `mpv.app` bundle; the executable is inside it.
pub(crate) fn normalize_mpv_selection(path: PathBuf) -> PathBuf {
    let is_bundle = path.is_dir() && path.extension().is_some_and(|ext| ext == "app");
    if is_bundle {
        path.join("Contents/MacOS/mpv")
    } else {
        path
    }
}

pub(crate) fn validate_mpv_path(value: Option<String>) -> Result<Option<String>, String> {
    let value = value.unwrap_or_default().trim().to_owned();
    if value.is_empty() {
        return Ok(None);
    }
    let path = Path::new(&value);
    if !path.is_absolute() || !path.is_file() {
        return Err("Укажите полный путь к существующему исполняемому файлу MPV".into());
    }
    Ok(Some(value))
}

pub(crate) fn load_player_settings(path: &Path) -> Result<PlayerSettings, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|err| format!("Не удалось прочитать settings.json: {err}")),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(PlayerSettings::default()),
        Err(err) => Err(format!("Не удалось открыть настройки: {err}")),
    }
}

pub(crate) fn save_player_settings(path: &Path, settings: &PlayerSettings) -> Result<(), String> {
    use std::io::Write;
    let settings = PlayerSettings {
        mpv_path: validate_mpv_path(settings.mpv_path.clone())?,
        mirror_embedded: settings.mirror_embedded,
    };
    let parent = path.parent().ok_or("Не определён каталог настроек")?;
    std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|err| err.to_string())?;
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|err| err.to_string())?;
    file.write_all(&bytes).map_err(|err| err.to_string())?;
    file.as_file().sync_all().map_err(|err| err.to_string())?;
    file.persist(path)
        .map_err(|err| format!("Не удалось сохранить настройки: {}", err.error))?;
    Ok(())
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
                    .map(|path| normalize_mpv_selection(path).to_string_lossy().into_owned())
                    .map_err(|err| err.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|err| err.to_string())?
}
