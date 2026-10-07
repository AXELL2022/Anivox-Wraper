# Anivox Desktop — Agent Guide

## Project overview

Anivox is a Tauri v2 desktop wrapper for `https://anivox.fun/`, with a Rust backend and JavaScript/CSS injected into the remote webview. Features include navigation controls, Discord Rich Presence, a system WireGuard VPN toggle, and external MPV playback. Windows and Linux have platform-specific implementations.

## Source map

- `src-tauri/src/lib.rs`: application setup, native window with separate `toolbar` and `content` webviews, VPN commands, Discord state, MPV launch, and the embedded `script` containing remote-page media hooks/video CSS.
- `ui/`: bundled local toolbar HTML/CSS/JS; navigation, VPN and MPV controls load independently of the remote website.
- `src-tauri/src/main.rs`: minimal entry point calling `anivox_lib::run()`; preserves the Windows release console suppression attribute.
- `src-tauri/Cargo.toml`: Rust 2021 crate and dependencies; library name is `anivox_lib`.
- `src-tauri/tauri.conf.json`: application identity, bundle settings, global Tauri bridge, and local asset directory (`../ui`). The main window and child webviews are created in Rust using Tauri's `unstable` multiwebview feature.
- `src-tauri/permissions/*.toml`: custom IPC permission definitions.
- `src-tauri/capabilities/default.json`: remote media/Discord permissions scoped to the `content` webview and `https://anivox.fun/*`. `toolbar.json` grants local browser/VPN controls only to `toolbar`.
- `src/`: local vanilla template/assets, not the remote site's UI. Its template `greet` invocation has no corresponding registered backend command.
- `ANIVOX-UA-48.conf`: WireGuard configuration embedded at compile time via `include_str!`.
- `.cargo/config.toml`: selects `rust-lld.exe` for Windows MSVC linking.
- `run.bat`: adds Bun's user installation to PATH and starts development.
- `CHANGELOG.md`, `INIT.md`, `GEMINI.md`: background documentation. Verify architectural claims against current code: some describe earlier implementations.
- `.github/workflows/build.yml`: CI/CD workflow for automated cross-platform builds (Windows and Linux) on push to `main` and release assets on tags.
- `backup_linux/`: ignored historical Linux files and build artifacts.


## Development commands

Run from the repository root. Bun is the documented JavaScript package manager and `bun.lock` is present.

```sh
bun install
bun run dev
bun run build
bun run build:arch
bun run install:arch
bun run tauri info
cargo check --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
node --test tests/toolbar.test.mjs
```

`package.json` defines `dev`, `build`, `build:arch`, `install:arch`, and `tauri`. Builds produce host-platform Tauri bundles and require the platform's Tauri/Rust prerequisites. Windows linking requires the configured `rust-lld.exe`.

## Implementation conventions

- Keep backend application logic in `lib.rs` and the entry point minimal. Follow nearby Rust and embedded JavaScript style; avoid unrelated formatting churn in the large raw string.
- Change `ui/` for toolbar design and controls; change the embedded `script` in `lib.rs` for media interception, video styling, keyboard shortcuts, and remote browser-side IPC calls.
- Register new IPC commands in `tauri::generate_handler!`, define their permissions under `src-tauri/permissions/`, and enable them in `capabilities/default.json`. Keep remote access scoped to the intended origin.
- Retain platform-specific `#[cfg(target_os = ...)]` branches when changing process execution or VPN behavior.
- VPN currently uses system WireGuard: `wireguard.exe` tunnel services on Windows, `nmcli` with a `wg-quick` fallback on Linux. Startup and window destruction attempt to disconnect an active named tunnel. There is no active `proxy.rs` or embedded wireproxy engine in the current source tree.
- On Windows, explicit VPN toggles retry access-denied WireGuard operations through PowerShell `Start-Process -Verb RunAs` (UAC). Only WireGuard is elevated. Automatic startup/exit cleanup is best-effort and does not request elevation; a non-elevated app may leave the tunnel active. UAC cancellation is reported in the toolbar.
- MPV launch prefers `F:\Mpv\mpv.exe` when present, otherwise `mpv` on PATH; playback arguments include stream metadata, headers, and start time. Account for these environment-specific paths when modifying or checking playback.
- WebView2 browser flags and injected video styling support the project's NVIDIA video-processing integration; preserve their intent during UI or player changes.
- `.gitignore` includes `/src/` and `backup_linux/`; check tracking status when adding or changing assets in these locations.

## Verification

- For Rust changes, run `cargo check --manifest-path src-tauri/Cargo.toml`; check formatting and distinguish existing formatting differences from changes introduced by the task.
- Toolbar state/error recovery tests use Node's built-in runner with a mocked Tauri bridge (`node --test tests/toolbar.test.mjs`). For injection changes, manually verify relevant behavior with `bun run dev`: navigation controls, remote IPC, VPN status/toggle, Discord updates, MPV handoff, and fullscreen/video layout as applicable.
- Discord, WireGuard, MPV, and the remote site are external runtime dependencies; report which integrations were actually exercised.
- Documentation-only edits do not require launching or building the application.
