//! Update check and verified download for the native shells.
//!
//! The release manifest (`latest.json`, the same one the Tauri updater reads)
//! carries the version, the notes from CHANGELOG and, per platform, the
//! download URL with a minisign signature made by the release key (the key in
//! `tauri.conf.json`). The shell shows the notes; right before acting it checks
//! the manifest again, and only the reviewed release ([`UpdateInfo::same_release`])
//! may be downloaded. A download is accepted only when its signature verifies
//! against [`UPDATE_PUBKEY`]; nothing is installed by this module.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(feature = "net")]
const MANIFEST_URL: &str =
    "https://github.com/AXELL2022/Anivox-Wraper/releases/latest/download/latest.json";
pub const RELEASE_PAGE: &str = "https://github.com/AXELL2022/Anivox-Wraper/releases/latest";
/// Release assets may only come from this repository's GitHub releases.
const DOWNLOAD_PREFIX: &str = "/AXELL2022/Anivox-Wraper/releases/download/";

/// Public half of the release signing key, identical to `plugins.updater.pubkey`
/// in `src-tauri/tauri.conf.json` (checked by `tests/updater_key.test.mjs`).
pub const UPDATE_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEEyQ0Y1MDczNjJDREM2MTEKUldRUnhzMWljMURQb2xoTEhGTlRONGNhQVRYQW9MV0dwdWR6Qzl1WkEvOEovVFRJazF6ZkRvSGkK";

/// Upper bound for a downloaded installer or package.
pub const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;

/// One downloadable file of a release: URL and base64 minisign signature.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Asset {
    pub url: String,
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    /// Keyed like the Tauri updater: `windows-x86_64-nsis`, `windows-x86_64`, `linux-x86_64`…
    #[serde(default)]
    pub platforms: BTreeMap<String, Asset>,
}

impl UpdateInfo {
    /// The first asset among `keys` (in order of preference).
    pub fn asset(&self, keys: &[&str]) -> Option<&Asset> {
        keys.iter().find_map(|key| self.platforms.get(*key))
    }

    /// The same release for these platform keys: version, URL and signature all
    /// match. A new check before downloading must pass this against the release
    /// whose notes the user reviewed.
    pub fn same_release(&self, other: &UpdateInfo, keys: &[&str]) -> bool {
        self.version == other.version && self.asset(keys) == other.asset(keys)
    }

    /// The file this build can download and install itself: an asset for
    /// [`platform_keys`] that links into this repository's releases.
    pub fn installable(&self) -> Option<&Asset> {
        self.asset(platform_keys())
            .filter(|asset| is_allowed_download_url(&asset.url))
    }
}

/// Result of [`download_reviewed`].
#[derive(Debug)]
pub enum Download {
    /// The reviewed release, downloaded and verified.
    Ready(std::path::PathBuf),
    /// The manifest no longer announces the reviewed release: show this one
    /// (or "no update") and ask again.
    Changed(Option<UpdateInfo>),
}

/// Manifest keys for this build, most specific first. The WinUI shell is
/// installed by an NSIS installer, like the old Tauri build it replaces.
pub fn platform_keys() -> &'static [&'static str] {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        &["windows-x86_64-nsis", "windows-x86_64"]
    } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        &["windows-aarch64-nsis", "windows-aarch64"]
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        &["linux-x86_64"]
    } else {
        &[]
    }
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

/// Only HTTPS links into this repository's GitHub releases are downloaded
/// (GitHub then redirects to its file storage).
pub fn is_allowed_download_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("github.com")
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path().starts_with(DOWNLOAD_PREFIX)
            && url.path().len() > DOWNLOAD_PREFIX.len()
    })
}

/// File name for a downloaded asset: the last URL segment, restricted to safe characters.
pub fn file_name(url: &str) -> String {
    let name: String = url::Url::parse(url)
        .ok()
        .and_then(|url| url.path_segments()?.next_back().map(str::to_owned))
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(120)
        .collect();
    if name.trim_matches('.').is_empty() {
        "anivox-update".to_owned()
    } else {
        name
    }
}

fn decode_base64_text(value: &str) -> Result<String, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .map_err(|_| "Подпись обновления повреждена".to_owned())?;
    String::from_utf8(bytes).map_err(|_| "Подпись обновления повреждена".to_owned())
}

/// The `version:` field of a Tauri signer trusted comment (`timestamp:…\tfile:…\tversion:…`).
fn signed_version(trusted_comment: &str) -> Option<&str> {
    trusted_comment
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"))
}

fn decode_signature(
    signature: &str,
    pubkey: &str,
) -> Result<(minisign_verify::PublicKey, minisign_verify::Signature), String> {
    let public_key = minisign_verify::PublicKey::decode(&decode_base64_text(pubkey)?)
        .map_err(|_| "Ключ проверки обновлений повреждён".to_owned())?;
    let signature = minisign_verify::Signature::decode(&decode_base64_text(signature)?)
        .map_err(|_| "Подпись обновления повреждена".to_owned())?;
    Ok((public_key, signature))
}

fn stream_error(err: minisign_verify::Error) -> String {
    match err {
        minisign_verify::Error::UnexpectedKeyId => "Обновление подписано чужим ключом".to_owned(),
        // Only prehashed signatures (what `tauri signer` makes) can be checked while downloading.
        _ => "Подпись обновления не поддерживается".to_owned(),
    }
}

/// After the signature itself verified: a version recorded in the (signed)
/// trusted comment must be the announced one.
fn check_signed_version(
    signature: &minisign_verify::Signature,
    version: &str,
) -> Result<(), String> {
    match signed_version(signature.trusted_comment()) {
        Some(signed) if signed.trim_start_matches('v') != version.trim_start_matches('v') => Err(
            format!("Подпись выдана для версии {signed}, а не {version}"),
        ),
        _ => Ok(()),
    }
}

/// Verifies `data` against a base64 manifest signature and the base64 release key.
pub fn verify_bytes(
    data: &[u8],
    signature: &str,
    pubkey: &str,
    version: &str,
) -> Result<(), String> {
    let (public_key, signature) = decode_signature(signature, pubkey)?;
    let mut stream = public_key.verify_stream(&signature).map_err(stream_error)?;
    stream.update(data);
    stream
        .finalize()
        .map_err(|_| "Подпись обновления не совпадает с файлом".to_owned())?;
    check_signed_version(&signature, version)
}

#[cfg(feature = "net")]
fn http_client(
    current: &str,
    timeout: Option<std::time::Duration>,
) -> Result<reqwest::Client, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut builder = reqwest::Client::builder().user_agent(format!("Anivox/{current}"));
    if let Some(timeout) = timeout {
        builder = builder.timeout(timeout);
    }
    builder.build().map_err(|err| err.to_string())
}

/// Reads the manifest; `current` is the running shell's version.
#[cfg(feature = "net")]
pub async fn check(current: &str) -> Result<Option<UpdateInfo>, String> {
    let client = http_client(current, Some(std::time::Duration::from_secs(15)))?;
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

/// Downloads the reviewed release's asset for this build into `dir` and returns
/// the file path once its signature verifies against [`UPDATE_PUBKEY`].
///
/// `progress(received, total)` is called as data arrives; setting `cancel`
/// stops the download. A failed, cancelled or unverified download leaves no file.
#[cfg(feature = "net")]
pub async fn download(
    release: &UpdateInfo,
    current: &str,
    dir: &std::path::Path,
    progress: impl FnMut(u64, Option<u64>),
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<std::path::PathBuf, String> {
    let asset = match release.asset(platform_keys()) {
        None => return Err("В выпуске нет файла для этой системы".into()),
        Some(asset) if !is_allowed_download_url(&asset.url) => {
            return Err("Недопустимый адрес файла обновления".into())
        }
        Some(asset) => asset,
    };
    let client = http_client(current, None)?;
    fetch_verified(
        &client,
        asset,
        &release.version,
        UPDATE_PUBKEY,
        dir,
        MAX_DOWNLOAD_BYTES,
        progress,
        cancel,
    )
    .await
}

/// Checks the manifest again and downloads only if it still announces the
/// `reviewed` release (same version, URL and signature); otherwise returns
/// [`Download::Changed`] so the shell can show the new notes. Files left in
/// `dir` by earlier downloads are removed first.
#[cfg(feature = "net")]
pub async fn download_reviewed(
    reviewed: &UpdateInfo,
    current: &str,
    dir: &std::path::Path,
    progress: impl FnMut(u64, Option<u64>),
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Download, String> {
    let latest = check(current).await?;
    match latest {
        Some(latest) if latest.same_release(reviewed, platform_keys()) => {
            clear_downloads(dir);
            download(&latest, current, dir, progress, cancel)
                .await
                .map(Download::Ready)
        }
        other => Ok(Download::Changed(other)),
    }
}

/// Removes old downloaded files (not subfolders) from the downloads folder.
pub fn clear_downloads(dir: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// The download itself, without the URL policy (tests serve files locally).
#[cfg(feature = "net")]
#[allow(clippy::too_many_arguments)]
async fn fetch_verified(
    client: &reqwest::Client,
    asset: &Asset,
    version: &str,
    pubkey: &str,
    dir: &std::path::Path,
    limit: u64,
    mut progress: impl FnMut(u64, Option<u64>),
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<std::path::PathBuf, String> {
    use std::io::Write;
    use std::sync::atomic::Ordering;

    let (public_key, signature) = decode_signature(&asset.signature, pubkey)?;
    let mut verifier = public_key.verify_stream(&signature).map_err(stream_error)?;
    std::fs::create_dir_all(dir)
        .map_err(|err| format!("Не удалось создать папку загрузки: {err}"))?;
    let mut file = tempfile::NamedTempFile::new_in(dir)
        .map_err(|err| format!("Не удалось создать файл загрузки: {err}"))?;

    let mut response = client
        .get(&asset.url)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|err| format!("Не удалось скачать обновление: {err}"))?;
    let total = response.content_length();
    if total.is_some_and(|total| total > limit) {
        return Err("Файл обновления слишком большой".into());
    }
    let mut received = 0u64;
    progress(received, total);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Загрузка отменена".into());
        }
        let Some(chunk) = response
            .chunk()
            .await
            .map_err(|err| format!("Загрузка прервалась: {err}"))?
        else {
            break;
        };
        received += chunk.len() as u64;
        if received > limit {
            return Err("Файл обновления слишком большой".into());
        }
        verifier.update(&chunk);
        file.write_all(&chunk)
            .map_err(|err| format!("Не удалось записать обновление: {err}"))?;
        progress(received, total);
    }
    if total.is_some_and(|total| total != received) {
        return Err("Загрузка прервалась".into());
    }
    verifier
        .finalize()
        .map_err(|_| "Подпись обновления не совпадает с файлом".to_owned())?;
    check_signed_version(&signature, version)?;
    file.flush()
        .map_err(|err| format!("Не удалось записать обновление: {err}"))?;
    let path = dir.join(file_name(&asset.url));
    file.persist(&path)
        .map_err(|err| format!("Не удалось сохранить обновление: {}", err.error))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Throwaway key generated with `tauri signer generate` for these tests only; the
    // signature was made by `tauri signer sign`, exactly like release artifacts.
    const TEST_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDQ0N0VFNzlBMUQ1QTdENEQKUldSTmZWb2RtdWQrUklJN01ZUURZcTRRWGxJblNISmlEK0ZxL1dvc3hwZEgxRWtvcm1CTUN4RnoK";
    const PAYLOAD: &[u8] = b"Anivox update test payload\n";
    const SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSTmZWb2RtdWQrUkdXZ3RFaVh4SXFRR2RmR2F4OHdMRVAyd1NiZ1dyVFdjZ0pqRkZheTN5QXlUMWUvQ1BzK3FLY2lYQUF2TStOYk5uN2FTYWpsaXVlMUR0OXUwVnZxOVFvPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk0NTcyNjA0CWZpbGU6cGF5bG9hZC5iaW4KWldjUnZhQzR4K3pMTGhheDArdTVGRnZGRi9hQmVHTXBhR2djTVBCZTAyWVZZREd4NTFRd2c2YURhZUxRdDBBcTUrL05Yb01haHUreTNJdUpVU2FUQWc9PQo=";

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

    fn release(version: &str, url: &str) -> UpdateInfo {
        let manifest = serde_json::json!({
            "version": version,
            "notes": "",
            "platforms": {
                "windows-x86_64": {"url": format!("{url}/old.exe"), "signature": "a"},
                "windows-x86_64-nsis": {"url": format!("{url}/setup.exe"), "signature": "b"},
                "linux-x86_64": {"url": format!("{url}/anivox.pkg.tar.zst"), "signature": "c"}
            }
        });
        serde_json::from_value(manifest).unwrap()
    }

    #[test]
    fn the_most_specific_platform_asset_is_chosen() {
        let info = release("1.0.0", "https://x");
        let nsis = info
            .asset(&["windows-x86_64-nsis", "windows-x86_64"])
            .unwrap();
        assert_eq!(nsis.signature, "b");
        assert_eq!(info.asset(&["linux-x86_64"]).unwrap().signature, "c");
        assert!(info.asset(&["darwin-aarch64"]).is_none());
        assert!(available(br#"{"version": "2.0.0"}"#, "1.0.0")
            .unwrap()
            .unwrap()
            .platforms
            .is_empty());
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            assert_eq!(platform_keys()[0], "windows-x86_64-nsis");
        }
    }

    #[test]
    fn only_the_reviewed_release_may_be_downloaded() {
        let keys = &["linux-x86_64"][..];
        let reviewed = release("1.0.0", "https://x");
        assert!(reviewed.same_release(&release("1.0.0", "https://x"), keys));
        assert!(!reviewed.same_release(&release("1.0.1", "https://x"), keys));
        assert!(!reviewed.same_release(&release("1.0.0", "https://y"), keys));
        let mut resigned = release("1.0.0", "https://x");
        resigned
            .platforms
            .get_mut("linux-x86_64")
            .unwrap()
            .signature = "z".into();
        assert!(!reviewed.same_release(&resigned, keys));
    }

    #[test]
    fn only_allowed_assets_for_this_build_are_installable() {
        let base = "https://github.com/AXELL2022/Anivox-Wraper/releases/download/v1.0.0";
        let ours = release("1.0.0", base);
        let foreign = release("1.0.0", "https://evil.test");
        if platform_keys().is_empty() {
            assert!(ours.installable().is_none());
        } else {
            assert!(ours.installable().is_some());
            assert!(foreign.installable().is_none());
        }
        let bare: UpdateInfo = serde_json::from_str(r#"{"version": "1.0.0"}"#).unwrap();
        assert!(bare.installable().is_none());
    }

    #[test]
    fn old_downloads_are_cleared_but_folders_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.exe"), b"x").unwrap();
        std::fs::create_dir(dir.path().join("keep")).unwrap();
        clear_downloads(dir.path());
        let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().collect();
        assert_eq!(left.len(), 1);
        assert!(left[0].path().is_dir());
        clear_downloads(&dir.path().join("missing"));
    }

    #[test]
    fn downloads_come_only_from_this_repositorys_releases() {
        let ok = "https://github.com/AXELL2022/Anivox-Wraper/releases/download/v1.0.0/Anivox_1.0.0_x64-setup.exe";
        assert!(is_allowed_download_url(ok));
        for bad in [
            "http://github.com/AXELL2022/Anivox-Wraper/releases/download/v1.0.0/a.exe",
            "https://github.com.evil.test/AXELL2022/Anivox-Wraper/releases/download/v1/a.exe",
            "https://github.com/Other/Anivox-Wraper/releases/download/v1/a.exe",
            "https://github.com:8443/AXELL2022/Anivox-Wraper/releases/download/v1/a.exe",
            "https://user@github.com/AXELL2022/Anivox-Wraper/releases/download/v1/a.exe",
            "https://github.com/AXELL2022/Anivox-Wraper/releases/download/",
            "https://github.com/AXELL2022/Anivox-Wraper/archive/main.zip",
            "not a url",
        ] {
            assert!(!is_allowed_download_url(bad), "{bad}");
        }
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(
            file_name("https://github.com/a/b/releases/download/v1/Anivox_1.0.0_x64-setup.exe"),
            "Anivox_1.0.0_x64-setup.exe"
        );
        assert_eq!(
            file_name("https://x/releases/download/v1/..%2F..%2Fevil"),
            "..2F..2Fevil"
        );
        assert_eq!(file_name("https://x/"), "anivox-update");
        assert_eq!(file_name("https://x/.."), "anivox-update");
    }

    #[test]
    fn a_tauri_signer_signature_verifies() {
        verify_bytes(PAYLOAD, SIGNATURE, TEST_PUBKEY, "1.0.0").unwrap();
        // The test signature records no version, like `tauri signer sign` on a file.
        verify_bytes(PAYLOAD, SIGNATURE, TEST_PUBKEY, "9.9.9").unwrap();
    }

    #[test]
    fn tampered_files_and_foreign_keys_are_rejected() {
        let mut tampered = PAYLOAD.to_vec();
        tampered[0] ^= 1;
        let err = verify_bytes(&tampered, SIGNATURE, TEST_PUBKEY, "1.0.0").unwrap_err();
        assert!(err.contains("не совпадает"), "{err}");
        let err = verify_bytes(PAYLOAD, SIGNATURE, UPDATE_PUBKEY, "1.0.0").unwrap_err();
        assert!(err.contains("чужим ключом"), "{err}");
        assert!(verify_bytes(PAYLOAD, "bm90IGEgc2lnbmF0dXJl", TEST_PUBKEY, "1.0.0").is_err());
        assert!(verify_bytes(PAYLOAD, "%%%", TEST_PUBKEY, "1.0.0").is_err());
        assert!(verify_bytes(PAYLOAD, SIGNATURE, "%%%", "1.0.0").is_err());
    }

    #[test]
    fn the_release_key_decodes() {
        let key = decode_base64_text(UPDATE_PUBKEY).unwrap();
        assert!(minisign_verify::PublicKey::decode(&key).is_ok());
    }

    #[test]
    fn a_signed_version_must_match_the_announced_one() {
        assert_eq!(
            signed_version("timestamp:1\tfile:a.exe\tversion:1.2.3"),
            Some("1.2.3")
        );
        assert_eq!(signed_version("timestamp:1\tfile:a.exe"), None);
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

    /// Downloads the current release's file for this build and verifies it with the real
    /// release key: `cargo test -- --ignored network`.
    #[cfg(feature = "net")]
    #[test]
    #[ignore = "needs the internet"]
    fn network_published_release_verifies_with_the_release_key() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let release = runtime.block_on(check("0.0.1")).unwrap().unwrap();
        if release.installable().is_none() {
            eprintln!("release {} has no file for this build", release.version);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let path = runtime
            .block_on(download(&release, "0.0.1", dir.path(), |_, _| {}, &cancel))
            .unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        eprintln!("verified {} ({size} bytes)", path.display());
        assert!(size > 0);
    }

    #[cfg(feature = "net")]
    mod download {
        use super::*;
        use std::sync::atomic::AtomicBool;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        /// Serves `body` once over plain HTTP with the given Content-Length.
        async fn serve(body: Vec<u8>, length: usize) -> String {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0u8; 1024];
                let _ = socket.read(&mut request).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
                let _ = socket.shutdown().await;
            });
            format!("http://{address}/releases/download/v1.0.0/Anivox_1.0.0_x64-setup.exe")
        }

        fn run<T>(future: impl std::future::Future<Output = T>) -> T {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(future)
        }

        async fn fetch(
            body: Vec<u8>,
            length: usize,
            limit: u64,
            dir: &std::path::Path,
            cancel: bool,
        ) -> (Result<std::path::PathBuf, String>, Vec<(u64, Option<u64>)>) {
            let url = serve(body, length).await;
            let asset = Asset {
                url,
                signature: SIGNATURE.into(),
            };
            let client = reqwest::Client::new();
            let mut seen = Vec::new();
            let cancel = AtomicBool::new(cancel);
            let result = fetch_verified(
                &client,
                &asset,
                "1.0.0",
                TEST_PUBKEY,
                dir,
                limit,
                |received, total| seen.push((received, total)),
                &cancel,
            )
            .await;
            (result, seen)
        }

        fn files(dir: &std::path::Path) -> Vec<String> {
            std::fs::read_dir(dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|entry| entry.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default()
        }

        #[test]
        fn a_verified_download_is_saved_under_its_name() {
            let dir = tempfile::tempdir().unwrap();
            let (result, seen) = run(fetch(
                PAYLOAD.to_vec(),
                PAYLOAD.len(),
                1024,
                dir.path(),
                false,
            ));
            let path = result.unwrap();
            assert_eq!(path.file_name().unwrap(), "Anivox_1.0.0_x64-setup.exe");
            assert_eq!(std::fs::read(&path).unwrap(), PAYLOAD);
            let total = Some(PAYLOAD.len() as u64);
            assert_eq!(seen.first(), Some(&(0, total)));
            assert_eq!(seen.last(), Some(&(PAYLOAD.len() as u64, total)));
            assert_eq!(files(dir.path()), ["Anivox_1.0.0_x64-setup.exe"]);
        }

        #[test]
        fn an_unverified_download_leaves_no_file() {
            let dir = tempfile::tempdir().unwrap();
            let mut tampered = PAYLOAD.to_vec();
            tampered[3] ^= 1;
            let (result, _) = run(fetch(tampered, PAYLOAD.len(), 1024, dir.path(), false));
            assert!(result.unwrap_err().contains("не совпадает"));
            assert!(files(dir.path()).is_empty());
        }

        #[test]
        fn oversized_truncated_and_cancelled_downloads_fail() {
            let dir = tempfile::tempdir().unwrap();
            let (result, _) = run(fetch(PAYLOAD.to_vec(), PAYLOAD.len(), 8, dir.path(), false));
            assert!(result.unwrap_err().contains("слишком большой"));
            let (result, _) = run(fetch(
                PAYLOAD[..10].to_vec(),
                PAYLOAD.len(),
                1024,
                dir.path(),
                false,
            ));
            assert!(result.is_err());
            let (result, _) = run(fetch(
                PAYLOAD.to_vec(),
                PAYLOAD.len(),
                1024,
                dir.path(),
                true,
            ));
            assert_eq!(result.unwrap_err(), "Загрузка отменена");
            assert!(files(dir.path()).is_empty());
        }

        #[test]
        fn foreign_links_are_refused_before_any_request() {
            let dir = tempfile::tempdir().unwrap();
            let mut info = release("1.0.0", "https://evil.test");
            for asset in info.platforms.values_mut() {
                asset.signature = SIGNATURE.into();
            }
            let cancel = AtomicBool::new(false);
            let result = run(download(&info, "0.5.0", dir.path(), |_, _| {}, &cancel));
            if platform_keys().is_empty() {
                assert!(result.is_err());
            } else {
                assert_eq!(result.unwrap_err(), "Недопустимый адрес файла обновления");
            }
        }
    }
}
