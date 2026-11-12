//! Player settings file (`settings.json`): format, validation, atomic save.

use std::path::Path;

#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlayerSettings {
    pub mpv_path: Option<String>,
    pub mirror_embedded: bool,
}

pub fn validate_mpv_path(value: Option<String>) -> Result<Option<String>, String> {
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

pub fn load_player_settings(path: &Path) -> Result<PlayerSettings, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|err| format!("Не удалось прочитать settings.json: {err}")),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(PlayerSettings::default()),
        Err(err) => Err(format!("Не удалось открыть настройки: {err}")),
    }
}

pub fn save_player_settings(path: &Path, settings: &PlayerSettings) -> Result<(), String> {
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
