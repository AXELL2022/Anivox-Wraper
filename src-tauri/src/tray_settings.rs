use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::AtomicBool;

pub(crate) struct TrayState(pub AtomicBool);

impl Default for TrayState {
    fn default() -> Self {
        Self(AtomicBool::new(true))
    }
}

#[derive(Serialize, Deserialize)]
struct TraySettings {
    minimize_to_tray: bool,
}

pub(crate) fn load(path: &Path) -> Result<bool, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<TraySettings>(&bytes)
            .map(|settings| settings.minimize_to_tray)
            .map_err(|err| err.to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(err) => Err(err.to_string()),
    }
}

pub(crate) fn save(path: &Path, enabled: bool) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().ok_or("Не определён каталог настроек")?;
    std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|err| err.to_string())?;
    let bytes = serde_json::to_vec_pretty(&TraySettings {
        minimize_to_tray: enabled,
    })
    .map_err(|err| err.to_string())?;
    file.write_all(&bytes).map_err(|err| err.to_string())?;
    file.as_file().sync_all().map_err(|err| err.to_string())?;
    file.persist(path).map_err(|err| err.error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_settings_preserve_close_to_tray_default() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("tray.json")).unwrap());
    }

    #[test]
    fn preference_survives_reload_and_can_be_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/tray.json");
        for enabled in [false, true, false] {
            save(&path, enabled).unwrap();
            assert_eq!(load(&path).unwrap(), enabled);
        }
    }

    #[test]
    fn malformed_settings_report_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tray.json");
        std::fs::write(&path, "invalid").unwrap();
        assert!(load(&path).is_err());
    }
}
