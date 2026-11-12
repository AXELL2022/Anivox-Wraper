//! Update check for the native shells.
//!
//! The release manifest (`latest.json`, the same one the Tauri updater reads)
//! carries the version and the notes from CHANGELOG. Native builds are never
//! replaced in place: after review the shell opens the release page. Before
//! that the version is checked again; if it changed, the new notes are shown
//! and need a fresh confirmation.

#[cfg(feature = "net")]
const MANIFEST_URL: &str =
    "https://github.com/AXELL2022/Anivox-Wraper/releases/latest/download/latest.json";
pub const RELEASE_PAGE: &str = "https://github.com/AXELL2022/Anivox-Wraper/releases/latest";

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct UpdateInfo {
    pub version: String,
    #[serde(default)]
    pub notes: String,
}

fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let core = value.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// True only for a well-formed version strictly newer than the running one.
pub fn is_newer(remote: &str, current: &str) -> bool {
    match (parse_version(remote), parse_version(current)) {
        (Some(remote), Some(current)) => remote > current,
        _ => false,
    }
}

/// `Some` when the manifest announces a newer version.
pub fn available(manifest: &[u8], current: &str) -> Result<Option<UpdateInfo>, String> {
    let mut info: UpdateInfo = serde_json::from_slice(manifest)
        .map_err(|err| format!("Некорректный манифест обновления: {err}"))?;
    info.version = info.version.trim().trim_start_matches('v').to_owned();
    Ok(is_newer(&info.version, current).then_some(info))
}

/// Reads the manifest; `current` is the running shell's version.
#[cfg(feature = "net")]
pub async fn check(current: &str) -> Result<Option<UpdateInfo>, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .user_agent(format!("Anivox/{current}"))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|err| err.to_string())?;
    let bytes = client
        .get(MANIFEST_URL)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|err| format!("Не удалось проверить обновления: {err}"))?
        .bytes()
        .await
        .map_err(|err| err.to_string())?;
    available(&bytes, current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_strictly_newer_versions_are_offered() {
        assert!(is_newer("0.4.4", "0.4.3"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("0.4.3", "0.4.3"));
        assert!(!is_newer("0.4.2", "0.4.3"));
        assert!(!is_newer("garbage", "0.4.3"));
        assert!(!is_newer("1.2", "0.4.3"));
    }

    #[test]
    fn manifest_notes_come_with_the_version() {
        let manifest =
            br###"{"version": "v9.0.0", "notes": "## [9.0.0]\n* new", "platforms": {}}"###;
        let info = available(manifest, "0.4.3").unwrap().unwrap();
        assert_eq!(info.version, "9.0.0");
        assert!(info.notes.contains("* new"));
        let same = br#"{"version": "0.4.3", "notes": ""}"#;
        assert_eq!(available(same, "0.4.3").unwrap(), None);
        assert!(available(b"<html>", "0.4.3").is_err());
    }

    /// Needs the internet: `cargo test -- --ignored network`.
    #[cfg(feature = "net")]
    #[test]
    #[ignore = "needs the internet"]
    fn network_release_manifest_is_readable() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let update = runtime.block_on(check("0.0.1")).unwrap();
        eprintln!("update: {update:?}");
    }
}
