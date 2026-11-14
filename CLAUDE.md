# CLAUDE.md

Руководство для агентов (Claude Code и др.) по репозиторию. Единственный файл с инструкциями. Если код расходится с текстом, прав код.

## Project overview

Anivox is a desktop wrapper for `https://anivox.fun/` with three builds on one Rust core (`src-tauri/anivox-core`) and one set of site scripts (`src-tauri/scripts`):

- **Windows:** `windows-winui/` (WinUI 3, C# + WebView2, Rust core through a C ABI).
- **Linux:** `linux-gtk/` (GTK4 + WebKitGTK 6.0).
- **macOS:** the minimal Tauri v2 app in `src-tauri/` (`macos.rs`).

The WinUI and GTK builds are still being tested; the next release ships them. The former Tauri build for Windows/Linux (toolbar webviews, `ui/` panels, `lib.rs` and its modules, capabilities, Arch `PKGBUILD`, Windows integration scripts) was retired: it lives in the git-ignored `backup_tauri/` folder of the main checkout and in git history (the last commit with it is in `backup_tauri/SOURCE_COMMIT`). Do not restore it into the tree unless asked.

Features: navigation controls, Discord Rich Presence with the Shikimori rating, external MPV playback with resume history, a Shikimori overlay, update checks with release notes, a connection check panel, session restore and a site recovery screen. macOS has only the site, menus, Discord and the Tauri updater.

## Source map

- `src-tauri/anivox-core/`: the shared platform-independent core (member of the `src-tauri` workspace, no Tauri types): `site_hosts`, `discord_presence`, `shikimori` (title ids, rating line, status decoration), `player_settings`, `tray_settings`, `mpv` (launch args, IPC protocol, playback state, resume history; feature `net`), `mpv_session` (launch/track/resume via the `MpvHost` trait that each shell implements; feature `net`); for the native GTK/WinUI shells: `site_ipc` (site command dispatcher `SiteIpc`/`SiteHost`, UI requests as `SiteEvent` through an `EventSink` closure; feature `net`), `connection` + `segments` (connection-check verdict `assess`/`fresh`, serializable `View`; `measure_internet` under `net`), `site_load` (load attempts `LoadInfo`, `document_report_matches`), `shikimori_overlay` (`start_url`, `page_activity`, `is_shikimori`), `updater` (`latest.json` parsing; `check` under `net`). Put new logic that needs no window/webview here; test with `cargo test --manifest-path src-tauri/Cargo.toml -p anivox-core`.
- `windows-winui/`: WinUI 3 shell (C#, .NET 10, Windows App SDK) with the features of the GTK build. Logic comes from `anivox-core` through `windows-winui/native` (`anivox-ffi`, own `[workspace]`, cdylib `anivox_ffi.dll`, built by the csproj via `cargo build --locked`; test with `cargo test --manifest-path windows-winui/native/Cargo.toml`): `ipc.rs` wires `anivox_core::site_ipc` to the callback (same site-facing commands as GTK), `session.rs` (`session-winui.json`), `lib.rs` (C ABI, JSON in/out, UI events through a callback, init script assembly in the former Tauri Windows order). `scripts/ipc-shim.js` exposes `window.__TAURI__.core.invoke` over `chrome.webview.postMessage`; `MainWindow.xaml.cs` accepts messages only from site origins and never replies to another origin. `Program.cs` (single instance via `AppInstance`), `TrayIcon.cs` (Shell_NotifyIcon, shared `tray.json`, close hides to tray), `AnivoxCore.cs` (P/Invoke), `MainWindow.Player.cs` (MPV button, port of `player_button.rs`), `MainWindow.Settings.cs` (settings flyout, port of `settings.rs`; settings/history FFI calls are shell-only, never site IPC). Flyouts over the site need `ShouldConstrainToRootBounds="False"`, otherwise they render under the WebView2 HWND. `MainWindow.Shikimori.cs` (overlay, port of GTK `shikimori.rs`; rules in `anivox_core::shikimori_overlay`): a second windowed WebView2 on the same environment, no Anivox IPC, close message accepted only from `https://shikimori.io`; while open the site HWND is hidden (not reloaded) and a dimmed `CapturePreviewAsync` snapshot is the backdrop, because XAML cannot cover an HWND. `WebViewHostFix` handles every `Chrome_WidgetWin_0` child. Player fullscreen never resizes the main window: `PlayerFullscreenWindow` (borderless popup over the monitor) takes the WebView2 via `CoreWebView2Controller.ParentWindow` and gives it back; resizing the main window instead showed it stretched with the site drawn at the old size. A WebView2 cannot redraw in step with a size change, so only finished frames are shown: the popup stays layered/transparent until two `requestAnimationFrame` (via CDP `Runtime.evaluate`) after the resize, the main window shows a `CapturePreviewAsync` snapshot (`PlayerSnapshot`) while the WebView2 is away, and on exit the site is redrawn at window size inside the transparent popup laid over the site area before it moves back. `MainWindow.Connection.cs`, `MainWindow.Recovery.cs` and `MainWindow.Updates.cs` port GTK `connection.rs`, `site_recovery.rs` and `updater.rs` (logic in `anivox-core`: `connection`, `site_load`, `updater`); the recovery screen appears only after the 25 s timeout or an error (showing it during every load would hide the site HWND), and updates download and run the signed installer (see the Updates convention). Settings/history/logs use the shared folders (`%APPDATA%`/`%LOCALAPPDATA%\com.yasin.anivox`). WebView2 must stay a windowed `CoreWebView2Controller` (child HWND over the `ContentHost` area), not the XAML `WebView2` control: visual hosting breaks RTX VSR. Keep `WebViewHostFix.cs` (clears `WS_EX_TRANSPARENT` on Chromium's `Intermediate D3D Window` and keeps the WebView2 HWND above WinUI's input bridge, microsoft-ui-xaml#10826): without it the site ignores all clicks on Windows 11. The title bar row height (48) must match `TitleBarHeightOption.Tall`, or the caption input window overlaps the site. Built in CI; locally `dotnet build windows-winui/Anivox.WinUI.csproj -c Release`. Installer: `windows-winui/installer/build-installer.ps1` (dotnet publish + NSIS `anivox.nsi`, per-user into `%LOCALAPPDATA%\Anivox`, same uninstall key `Anivox` as the old Tauri build, accepts the Tauri updater flags `/P /S /UPDATE /R`, removes a Tauri install with its uninstaller in update mode, never deletes user data); keep `EnableMsixTooling` in the csproj or the published app crashes without `resources.pri`. Test it only through `test-installer.ps1`, which uses the product key `AnivoxInstallerTest`: running a real installer on the developer's machine replaces their installed Anivox.
- `linux-gtk/`: standalone GTK4 + WebKitGTK 6.0 shell (own `[workspace]`, not built by `cargo test --workspace`; build it on Linux with `cargo build --manifest-path linux-gtk/Cargo.toml`, package it with `makepkg -fi` in `linux-gtk/arch`, which builds from a snapshot in `$srcdir`). Every panel is a native GTK widget: `session.rs` (size/maximized/last page in `session-gtk.json`, restore-paused guard), `tray.rs` (StatusNotifierItem via `ksni`, close to tray, shared `tray.json`), `settings.rs`, `connection.rs`, `site_recovery.rs`, `shikimori.rs` (GTK widgets only; their logic is in `anivox-core`; overlay WebView with its own content manager whose only handler closes the overlay; Discord page-kind status), `updater.rs` (review, verified download into `~/.cache/com.yasin.anivox/updates`, `pkexec pacman -U --noconfirm --ask=4` so the package replaces the Tauri `anivox` package, restart after the old process exits), `player_button.rs`. Site scripts reach `ipc.rs` (wires `anivox_core::site_ipc` to an async channel) through the `window.__TAURI__.core.invoke` shim (`scripts/ipc-shim.js`), only from `anivox.fun`/`anivox.me`; site-facing commands: `set_discord_rpc`, `open_in_mpv`, `get_mpv_state`, `get_playback_resume`, `report_player_status`, `content_fullscreen`, `report_segment_stats`, `get_site_document_generation`, `site_document_state`. Settings, history clearing and the MPV path are reachable only from the native settings popover. `vblank.rs` + `GSK_RENDERER=gl` lift the 60 FPS cap on NVIDIA; do not remove. `src/main.rs`/`src/ipc.rs` use CRLF line endings. Shares `settings.json`, `tray.json` and playback history with the other builds (`com.yasin.anivox`).
- `src-tauri/`: minimal macOS Tauri app. `src/main.rs` runs `macos::run()` on macOS and only prints a notice elsewhere (keeps `cargo test --workspace` for the core working on Windows/Linux); `src/macos.rs` (single WKWebView, native menus, origin-scoped `set_discord_rpc`, reviewed updater), `src/discord_presence.rs` (re-export from the core), `tauri.conf.json` + `tauri.macos.conf.json`, `capabilities/macos-discord.json`, `permissions/discord.toml`. Details in `docs/MACOS-EXPERIMENTAL.md`.
- `src-tauri/scripts/`: scripts injected into the site page by every build: `inject-common.js` (MPV media hooks and Discord presence), `player-fullscreen.js` (native fullscreen sync and double-click toggle), `common-styles.js`, `inject-windows.js` (RTX VSR DirectComposition CSS, WinUI), `inject-linux.js` (WebKitGTK workarounds, GTK), `inject-macos.js` (macOS), `mpv-session.js`, `restore-paused.js`, `site-lifecycle.js`, `segment-monitor.js`. Each native shell has its own `scripts/ipc-shim.js` exposing `window.__TAURI__.core.invoke`.
- `src-tauri/icons/`: application icons (also used by WinUI and GTK).
- `tests/`: Node tests for the site scripts, macOS boundaries, the updater manifest script and version sync.
- `.cargo/config.toml`: selects `rust-lld.exe` for Windows MSVC linking (WinUI native core).
- `.github/workflows/build.yml`: CI on the self-hosted Windows runner: JS tests, formatting, core and `anivox-ffi` tests, WinUI installer (artifact), GTK build and tests inside the WSL distribution `cachyos`. `.github/workflows/release.yml` (tag `vX.Y.Z` or manual run with an existing tag, same runner): checks the tag against the version, runs the tests, extracts the notes, builds the WinUI installer and the GTK package (`makepkg` in WSL), signs both with `TAURI_SIGNING_PRIVATE_KEY` through `npx @tauri-apps/cli signer sign`, generates `latest.json` (`windows-x86_64` = WinUI installer, also for old Tauri clients; `linux-x86_64` = GTK package), verifies every signature with the `verify_release` example of `anivox-core` and publishes the GitHub release. macOS is not part of the release. `.github/workflows/macos-minimal.yml`: macOS artifacts on GitHub-hosted runners.
- `scripts/generate-updater-manifest.mjs`: release notes and `latest.json`; `scripts/runner-on-demand.ps1`: starts the self-hosted runner from `.githooks/pre-push`.
- `CHANGELOG.md`: release history; its sections feed GitHub Releases and `latest.json`.

## Development commands

Run from the repository root.

```sh
node --test tests/*.test.mjs
cargo test --locked --manifest-path src-tauri/Cargo.toml -p anivox-core
cargo test --locked --manifest-path windows-winui/native/Cargo.toml
dotnet build windows-winui/Anivox.WinUI.csproj -c Release
cargo build --locked --manifest-path linux-gtk/Cargo.toml   # Linux or WSL
cargo test --locked --manifest-path linux-gtk/Cargo.toml    # Linux or WSL
cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check
cargo fmt --manifest-path windows-winui/native/Cargo.toml -- --check
cargo fmt --manifest-path linux-gtk/Cargo.toml -- --check
bun install --frozen-lockfile && bun run tauri build        # macOS only
```

On Windows, GTK builds and tests run in WSL (`wsl -d cachyos`); keep `CARGO_TARGET_DIR` inside the Linux file system. A running WinUI instance locks `windows-winui/bin/.../anivox-winui.exe`; build test variants with `-p:OutDir=<elsewhere>` instead of killing the user's instance.

## Implementation conventions

- Put logic that needs no window/webview into `anivox-core` and use it from both native shells; shells keep only UI glue. Network code goes under the core feature `net`.
- Site-facing commands live in `anivox_core::site_ipc` (same set in GTK and WinUI). Shells accept invoke messages only from the site origins (`anivox.fun`, `anivox.me`) and never reply to other origins. Settings, history clearing and the MPV path are shell-only (native settings UI), never site IPC.
- Shikimori has no Anivox/MPV access: only Shikimori itself (`https://shikimori.io`) may close its overlay. Keep HTTPS navigation and engine-managed login popups for social sign-in; sign-in providers get no IPC. Closing destroys the overlay view; reopening starts at the same title (`/anime/<id>` on the site) or the Shikimori home page.
- MPV launch uses the executable from settings, otherwise `mpv` on PATH. Settings are saved atomically; per-launch logs go to the local data `logs` folder. Preserve metadata, headers, start time and subtitles. Discord uses the active MPV session while its JSON IPC is ready; embedded snapshots are retained for fallback. Optional `mirrorEmbedded` follows real MPV state on the same episode with mute/minimum quality; never synthesize viewing events. Resume history is local, atomic and keyed by `anime:episode:translation`.
- WebView2 browser flags (`BrowserArgs` in WinUI) and `inject-windows.js` support NVIDIA RTX VSR/TrueHDR; preserve their intent. WinUI WebView2 must stay a windowed `CoreWebView2Controller`.
- Updates: `anivox_core::updater` re-checks `latest.json` right before acting and proceeds only with the reviewed release (same version, URL and signature); downloads come only from this repository's GitHub releases and are kept only when the minisign signature verifies against `UPDATE_PUBKEY` (the Tauri updater key, `tests/updater_key.test.mjs`). WinUI then runs the NSIS installer with `/P /UPDATE` and exits; without an installer asset it opens the release page. GTK installs the verified package with `pkexec pacman -U` and restarts; `linux-gtk/arch/PKGBUILD` provides/conflicts/replaces `anivox`. The core's HTTPS needs the shells' `rustls-no-provider` feature, so network tests run through a shell workspace (`cargo test --manifest-path windows-winui/native/Cargo.toml -p anivox-core -- --ignored network`).
- Retain platform-specific `#[cfg(target_os = ...)]` branches when changing process execution.
- User-facing docs (README, CHANGELOG) are written in Russian.

## Verification

- For Rust changes run the tests and `cargo fmt -- --check` for every affected manifest; CI uses `--locked`, so update lock files with cargo when dependencies change.
- For WinUI changes build with `dotnet build`; for GTK changes build and test in WSL. Report what was actually exercised: Discord, MPV and the remote site are external runtime dependencies.
- macOS cannot be built on Windows (Apple toolchain needed); rely on `macos-minimal.yml` and say so.
- Documentation-only edits do not require building.

## Release

The tag `vX.Y.Z` takes its notes from the `## [X.Y.Z]` section of `CHANGELOG.md` via `scripts/generate-updater-manifest.mjs`; a missing or empty section stops the release. `release.yml` publishes; before the first WinUI release test the Tauri migration by hand (`Anivox_X_x64-setup.exe /P /UPDATE /R` over an installed Tauri build). Gate a release with `cargo run -p anivox-core --example verify_release -- <latest.json> <assets dir>` (`--pubkey` for dry runs with a test key). `linux-gtk/arch/PKGBUILD` and `linux-gtk/data/*` stay LF (`.gitattributes`): makepkg rejects CRLF. Keep `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`, `linux-gtk/Cargo.toml`, `linux-gtk/Cargo.lock`, `linux-gtk/arch/PKGBUILD`, `windows-winui/Anivox.WinUI.csproj`, `windows-winui/native/Cargo.toml` and `windows-winui/native/Cargo.lock` in sync; `tests/version_sync.test.mjs` fails when they diverge. The self-hosted runner (`actions-runner/`, git-ignored) runs as the user, which gives CI access to the WSL distribution.

## Архитектура

### Сборки и данные

Идентификатор приложения — `com.yasin.anivox`: настройки (`settings.json`, `tray.json`), история (`playback-history.json`) и журналы MPV общие для всех сборок; сеанс окна у каждой свой (`session-gtk.json`, `session-winui.json`). Профиль WebView2 у WinUI — `%LOCALAPPDATA%\com.yasin.anivox.winui\WebView2`. Крестик скрывает окно в трей, «Выход» в меню трея завершает приложение; повторный запуск показывает уже открытое окно.

### Скрипты сайта

Порядок init-скрипта: защищённый токеном `restore-paused.js` (восстановленная страница не запускает видео до жеста пользователя), затем платформенный скрипт, `mpv-session.js`, `inject-common.js`, `player-fullscreen.js`, `common-styles.js`, затем `site-lifecycle.js` (сообщает о готовности документа, не о буферизации) и `segment-monitor.js`.

### Проверка соединения

`segment-monitor.js` пассивно наблюдает за XHR/fetch видеосегментов (`.ts`, `.m4s`, `.mp4`…) только на сайте и только в верхнем документе, ничего не меняя; раз в секунду вызывает `report_segment_stats`. Панель раз в секунду читает статистику, отдельно меряет скорость скачиванием с `speed.cloudflare.com` и показывает вердикт из `anivox_core::connection` (ниже 10 Мбит/с — красным; данные старше 4 с считаются отсутствующими).

### Discord

Встроенный плеер сообщает статус через `set_discord_rpc`; активная сессия MPV имеет приоритет, при её завершении возвращается последний статус встроенного плеера. Перед статусом добавляется рейтинг Shikimori из публичного API (кэш, сбой запоминается на 5 минут). Пока открыт оверлей Shikimori и MPV не играет, показывается только тип страницы («Смотрит топ аниме», «Листает Shikimori»); имена профилей и поисковые запросы в Discord не попадают.

### Экран восстановления

Навигация начинает новую попытку загрузки (`anivox_core::site_load`); завершить её может только отчёт `site-lifecycle.js` для текущей страницы и попытки. Через 25 с без готовности показывается экран с повтором, зеркалом и главной; таймаут не доказывает сбой сети, и медленная страница может догрузиться.
