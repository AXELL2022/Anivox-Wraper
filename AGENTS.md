# Anivox Desktop — Agent Guide

## Project overview

Anivox is a Tauri v2 desktop wrapper for `https://anivox.fun/`, with a Rust backend and JavaScript/CSS injected into the remote webview. Features include navigation controls, Discord Rich Presence, a system WireGuard VPN toggle, and external MPV playback. Windows and Linux have platform-specific implementations.

## Source map

- `src-tauri/src/lib.rs`: application setup, native window with separate `toolbar` and `content` webviews, VPN commands, Discord state, MPV launch, and initialization script assembly via `concat!` and `include_str!`.
- `src-tauri/scripts/`: remote webview injection scripts separated by platform (`inject-common.js`, `inject-windows.js`, `inject-linux.js`), including MPV media hooks, fullscreen handlers, RTX VSR DirectComposition CSS, and WebKitGTK buffer/pacer workarounds.
- `ui/`: bundled local toolbar HTML/CSS/JS and Shikimori overlay controls (`shikimori.html`, `.css`, `.js`). Windows uses an independent toolbar; Linux injects it into the remote page.
- Shikimori opens lazily in `shikimori` with local controls/backdrop in `shikimori-overlay`. Closing destroys both views; reopening creates a new Shikimori page at its homepage. Windows positions child webviews; Linux reparents existing WebKit widgets into a native `GtkOverlay` while keeping the Anivox view alive. Keep browser arguments consistent across Windows webviews sharing a WebView2 profile.
- `src-tauri/scripts/mpv-session.js`: same-episode embedded mirroring driven by native MPV snapshots; loaded before inject-common.js on all platforms.
- `src-tauri/src/main.rs`: minimal entry point calling `anivox_lib::run()`; preserves the Windows release console suppression attribute.
- `src-tauri/Cargo.toml`: Rust 2021 crate and dependencies; library name is `anivox_lib`.
- `src-tauri/tauri.conf.json`: application identity, bundle settings, global Tauri bridge, and local asset directory (`../ui`). The main window and child webviews are created in Rust using Tauri's `unstable` multiwebview feature.
- `src-tauri/permissions/*.toml`: custom IPC permission definitions.
- `src-tauri/capabilities/default.json`: remote media/Discord permissions scoped to the `content` webview and `https://anivox.fun/*`. `toolbar.json` grants local browser/VPN controls only to `toolbar`.
- `ui/settings.html` and `ui/update-review.html`: transparent local child overlay webviews for settings and release notes, managed by `src-tauri/src/local_overlay.rs`. Only one panel opens at a time, anchored to its toolbar button. Close the child webview, NEVER its parent window; keep local origin/label guards and reviewed-version installation. `ui/local-overlay.js` handles positioning, outside click and focus trapping.
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
cargo test --manifest-path src-tauri/Cargo.toml
node --test tests/toolbar.test.mjs
```

`package.json` defines `dev`, `build`, `build:arch`, `install:arch`, and `tauri`. Builds produce host-platform Tauri bundles and require the platform's Tauri/Rust prerequisites. Windows linking requires the configured `rust-lld.exe`.

## Implementation conventions

- Keep backend application logic in `lib.rs` and the entry point minimal.
- Change `ui/` for toolbar design and controls; change `src-tauri/scripts/inject-common.js` for cross-platform remote hooks, `src-tauri/scripts/inject-windows.js` for Windows/VSR DirectComposition styles, and `src-tauri/scripts/inject-linux.js` for WebKitGTK-specific engine/buffer optimizations.
- Register new IPC commands in `tauri::generate_handler!`, define their permissions under `src-tauri/permissions/`, and enable them in `capabilities/default.json`. Keep remote access scoped to the intended origin.
- Retain platform-specific `#[cfg(target_os = ...)]` branches when changing process execution or VPN behavior.
- Shikimori has no Anivox/VPN/MPV access: its origin-scoped capability only permits closing its own overlay on Escape. Navigation controls belong to the local overlay and application toolbar.
- Keep Shikimori's HTTPS navigation and engine-managed login popups enabled for social sign-in. External login providers receive no application IPC permissions; do not broaden the origin-scoped capability to enable OAuth.
- VPN currently uses system WireGuard: `wireguard.exe` tunnel services on Windows, `nmcli` with a `wg-quick` fallback on Linux. Startup and window destruction attempt to disconnect an active named tunnel. There is no active `proxy.rs` or embedded wireproxy engine in the current source tree.
- On Windows, explicit VPN toggles retry access-denied WireGuard operations through PowerShell `Start-Process -Verb RunAs` (UAC). Only WireGuard is elevated. Automatic startup/exit cleanup is best-effort and does not request elevation; a non-elevated app may leave the tunnel active. UAC cancellation is reported in the toolbar.
- MPV launch uses the executable selected in local settings, otherwise `mpv` on PATH. Settings are saved atomically under Tauri app_config_dir; per-launch logs go to app_local_data_dir/logs. Preserve metadata, headers, start time and subtitles. Only the local settings webview may choose or save an executable path. Discord uses the active MPV session while its local JSON IPC is ready; embedded snapshots are retained for fallback. Optional mirrorEmbedded follows real MPV state on the same episode with mute/minimum quality; never synthesize viewing events. Resume history is local, atomic and keyed by anime:episode:translation.
- WebView2 browser flags and injected video styling support the project's NVIDIA video-processing integration; preserve their intent during UI or player changes.
- The root `src/` template and unused `src-tauri/binaries/wireproxy.exe` have been removed. `backup_linux/` remains ignored historical material.
- Update installation belongs only to local `update-review` and requires the reviewed version. Remote content and the toolbar may open that window but cannot install directly. Release notes from CHANGELOG must feed both GitHub Release and latest.json.

## Verification

- For Rust changes, run `cargo check --manifest-path src-tauri/Cargo.toml`; check formatting and distinguish existing formatting differences from changes introduced by the task.
- Toolbar state/error recovery tests use Node's built-in runner with a mocked Tauri bridge (`node --test tests/toolbar.test.mjs`). For injection changes, manually verify relevant behavior with `bun run dev`: navigation controls, remote IPC, VPN status/toggle, Discord updates, MPV handoff, and fullscreen/video layout as applicable.
- Run `node --test tests/*.test.mjs` for all UI tests. Verify Shikimori opens above Anivox, unloads via the button/Escape/backdrop, starts at its homepage on reopening, and follows window resizing without reloading Anivox.
- Discord, WireGuard, MPV, and the remote site are external runtime dependencies; report which integrations were actually exercised.
- Documentation-only edits do not require launching or building the application.
- For Windows taskbar icon changes, build a release executable and run `powershell -NoProfile -File tests/windows_taskbar_icon.ps1 -Exe src-tauri/target/release/anivox.exe`. This launches a test instance and checks that both `ICON_SMALL` and `ICON_BIG` contain valid window icons; the script stops only its own instance.
