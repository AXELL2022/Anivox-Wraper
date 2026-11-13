//! Release gate: checks a generated `latest.json` against the files that will be published,
//! exactly as the apps will check them after download.
//!
//!   cargo run -p anivox-core --example verify_release -- <latest.json> <assets dir> [--pubkey <b64>]
//!
//! For every platform entry the URL must point into this repository's release for the
//! manifest version, the file must exist in the assets folder and its signature must verify
//! against the release key ([`UPDATE_PUBKEY`], or `--pubkey` for dry runs with a test key).
//! Exits with status 1 on the first problem.

use anivox_core::updater::{
    file_name, is_allowed_download_url, verify_bytes, UpdateInfo, UPDATE_PUBKEY,
};
use std::path::PathBuf;
use std::process::ExitCode;

fn run() -> Result<usize, String> {
    let mut args = std::env::args().skip(1);
    let manifest = PathBuf::from(
        args.next()
            .ok_or("usage: verify_release <latest.json> <assets dir> [--pubkey <b64>]")?,
    );
    let assets = PathBuf::from(args.next().ok_or("missing assets dir")?);
    let pubkey = match (args.next().as_deref(), args.next()) {
        (Some("--pubkey"), Some(key)) => key,
        (None, _) => UPDATE_PUBKEY.to_owned(),
        _ => return Err("unexpected arguments".into()),
    };
    let text = std::fs::read(&manifest).map_err(|err| format!("{}: {err}", manifest.display()))?;
    let info: UpdateInfo =
        serde_json::from_slice(&text).map_err(|err| format!("latest.json: {err}"))?;
    if info.notes.trim().is_empty() {
        return Err("latest.json has no release notes".into());
    }
    if info.platforms.is_empty() {
        return Err("latest.json lists no platforms".into());
    }
    let release_path = format!("/releases/download/v{}/", info.version);
    for (platform, asset) in &info.platforms {
        if !is_allowed_download_url(&asset.url) || !asset.url.contains(&release_path) {
            return Err(format!(
                "{platform}: URL is not in release v{}: {}",
                info.version, asset.url
            ));
        }
        let path = assets.join(file_name(&asset.url));
        let data =
            std::fs::read(&path).map_err(|err| format!("{platform}: {}: {err}", path.display()))?;
        verify_bytes(&data, &asset.signature, &pubkey, &info.version)
            .map_err(|err| format!("{platform}: {}: {err}", path.display()))?;
        println!("ok  {platform}: {} ({} bytes)", path.display(), data.len());
    }
    Ok(info.platforms.len())
}

fn main() -> ExitCode {
    match run() {
        Ok(count) => {
            println!("{count} release file(s) verified");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("verify_release: {err}");
            ExitCode::FAILURE
        }
    }
}
