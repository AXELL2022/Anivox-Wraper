# CLAUDE.md

Руководство для агентов (Claude Code и др.) по репозиторию. Единственный файл с инструкциями: он заменил `AGENTS.md`, `GEMINI.md` и `INIT.md`. Если код расходится с текстом, прав код (актуально для версии 0.4.0).

## Project overview

Anivox is a Tauri v2 desktop wrapper for `https://anivox.fun/`, with a Rust backend and JavaScript/CSS injected into the remote webview. Features include navigation controls, Discord Rich Presence, external MPV playback, a Shikimori overlay, update review/installation and a connection check panel. Windows and Linux have platform-specific implementations.

## Source map

- `src-tauri/src/lib.rs`: application setup, native window with separate `toolbar` and `content` webviews, window/Shikimori overlay IPC commands and initialization script assembly via `concat!` and `include_str!`.
- `src-tauri/anivox-core/`: platform-independent crate without Tauri types (workspace member): `site_hosts`, `discord_presence`, `shikimori` (title ids, rating line, status decoration), `player_settings`, `tray_settings`, `mpv` (launch args, IPC protocol, playback state, resume history; feature `net`), `mpv_session` (launch/track/resume via the `MpvHost` trait that each shell implements; feature `net`); for the native GTK/WinUI shells: `site_ipc` (site command dispatcher `SiteIpc`/`SiteHost`, UI requests as `SiteEvent` through an `EventSink` closure; feature `net`), `connection` + `segments` (connection-check verdict `assess`/`fresh`, serializable `View`; `measure_internet` under `net`), `site_load` (load attempts `LoadInfo`, `document_report_matches`), `shikimori_overlay` (`start_url`, `page_activity`, `is_shikimori`), `updater` (`latest.json` parsing; `check` under `net`). The shell modules in `src-tauri/src/` re-export these items and keep only Tauri glue (commands, paths, state). Put new logic that needs no window/webview here; run `cargo test --workspace`.
- `linux-gtk/`: experimental standalone GTK4 + WebKitGTK 6.0 shell (own `[workspace]`, not built by `cargo test --workspace`; build it on Linux with `cargo build --manifest-path linux-gtk/Cargo.toml`, package it with `makepkg -fi` in `linux-gtk/arch`, which builds from a snapshot in `$srcdir`). Feature parity with the Tauri build, but every local panel is a native GTK widget, not a local webview: `session.rs` (size/maximized/last page in `session-gtk.json`, restore-paused guard), `tray.rs` (StatusNotifierItem via `ksni`, close to tray, shared `tray.json`), `settings.rs`, `connection.rs`, `site_recovery.rs`, `shikimori.rs` (GTK widgets only; their logic is in `anivox-core`; overlay WebView with its own content manager whose only handler closes the overlay; Discord page-kind status), `updater.rs` (reads `latest.json`, opens the release page for the pacman package), `player_button.rs`. Site scripts reach `ipc.rs` (wires `anivox_core::site_ipc` to an async channel) through the `window.__TAURI__.core.invoke` shim (`scripts/ipc-shim.js`), only from `anivox.fun`/`anivox.me`; site-facing commands: `set_discord_rpc`, `open_in_mpv`, `get_mpv_state`, `get_playback_resume`, `report_player_status`, `content_fullscreen`, `report_segment_stats`, `get_site_document_generation`, `site_document_state`. Settings, history clearing and the MPV path are reachable only from the native settings popover. `vblank.rs` + `GSK_RENDERER=gl` lift the 60 FPS cap on NVIDIA; do not remove. `src/main.rs`/`src/ipc.rs` use CRLF line endings. Shares `settings.json`, `tray.json` and playback history with the Tauri build (`com.yasin.anivox`).
- `windows-winui/`: experimental WinUI 3 shell (C#, .NET 10, Windows App SDK) with the features of the Tauri/GTK builds. Logic comes from `anivox-core` through `windows-winui/native` (`anivox-ffi`, own `[workspace]`, cdylib `anivox_ffi.dll`, built by the csproj via `cargo build --locked`; test with `cargo test --manifest-path windows-winui/native/Cargo.toml`): `ipc.rs` wires `anivox_core::site_ipc` to the callback (same site-facing commands as GTK), `session.rs` (`session-winui.json`), `lib.rs` (C ABI, JSON in/out, UI events through a callback, init script assembly in the Tauri Windows order). `scripts/ipc-shim.js` exposes `window.__TAURI__.core.invoke` over `chrome.webview.postMessage`; `MainWindow.xaml.cs` accepts messages only from site origins and never replies to another origin. `Program.cs` (single instance via `AppInstance`), `TrayIcon.cs` (Shell_NotifyIcon, shared `tray.json`, close hides to tray), `AnivoxCore.cs` (P/Invoke), `MainWindow.Player.cs` (MPV button, port of `player_button.rs`), `MainWindow.Settings.cs` (settings flyout, port of `settings.rs`; settings/history FFI calls are shell-only, never site IPC). Flyouts over the site need `ShouldConstrainToRootBounds="False"`, otherwise they render under the WebView2 HWND. `MainWindow.Shikimori.cs` (overlay, port of GTK `shikimori.rs`; rules in `anivox_core::shikimori_overlay`): a second windowed WebView2 on the same environment, no Anivox IPC, close message accepted only from `https://shikimori.io`; while open the site HWND is hidden (not reloaded) and a dimmed `CapturePreviewAsync` snapshot is the backdrop, because XAML cannot cover an HWND. `WebViewHostFix` handles every `Chrome_WidgetWin_0` child. `MainWindow.Connection.cs`, `MainWindow.Recovery.cs` and `MainWindow.Updates.cs` port GTK `connection.rs`, `site_recovery.rs` and `updater.rs` (logic in `anivox-core`: `connection`, `site_load`, `updater`); the recovery screen appears only after the 25 s timeout or an error (showing it during every load would hide the site HWND), and updates open the release page instead of installing. Settings/history/logs use the Tauri folders (`%APPDATA%`/`%LOCALAPPDATA%\com.yasin.anivox`). WebView2 must stay a windowed `CoreWebView2Controller` (child HWND over the `ContentHost` area), not the XAML `WebView2` control: visual hosting breaks RTX VSR. Keep `WebViewHostFix.cs` (clears `WS_EX_TRANSPARENT` on Chromium's `Intermediate D3D Window` and keeps the WebView2 HWND above WinUI's input bridge, microsoft-ui-xaml#10826): without it the site ignores all clicks on Windows 11. The title bar row height (48) must match `TitleBarHeightOption.Tall`, or the caption input window overlaps the site. Not part of the Tauri workspace or CI; build with `dotnet build windows-winui/Anivox.WinUI.csproj -c Release`.
- `src-tauri/src/main.rs`: minimal entry point calling `anivox_lib::run()`; preserves the Windows release console suppression attribute.
- `src-tauri/src/` modules: `discord.rs` (Rich Presence and the Shikimori rating line), `mpv.rs` (MPV launch, IPC tracking and playback history), `player_settings.rs` (`settings.json` and the settings commands), `updater.rs` (update check/install commands), `session.rs` (window geometry and last-page persistence; read its module doc for the required call order around window creation, tray hiding and exit), `site_recovery.rs` (native loading watchdog; a timeout is not proof of network failure), `tray_settings.rs` (close-to-tray state), `local_overlay.rs` (local dropdown panels). Rust tests live in `*_tests.rs` files next to `lib.rs` and in inline `#[cfg(test)]` modules.
- `src-tauri/scripts/`: remote webview injection scripts: `inject-common.js` (MPV media hooks and Discord presence), `player-fullscreen.js` (native fullscreen sync and double-click toggle), `common-styles.js` (scroll/backdrop CSS), `inject-windows.js` (RTX VSR DirectComposition CSS), `inject-linux.js` (WebKitGTK buffer/pacer workarounds), `mpv-session.js` (same-episode embedded mirroring driven by native MPV snapshots; loaded before `inject-common.js` on all platforms), `restore-paused.js`, `site-lifecycle.js`, `segment-monitor.js`, and `shikimori.js` (Shikimori webview only).
- `ui/`: bundled local toolbar HTML/CSS/JS, local panels (`settings`, `update-review`, `connection`), Shikimori overlay controls and the site recovery screen. Windows uses an independent toolbar (`index.html`); Linux injects `toolbar-fragment.html` into the remote page.
- `src-tauri/Cargo.toml`: Rust 2021 crate and dependencies; library name is `anivox_lib`.
- `src-tauri/tauri.conf.json`: application identity, bundle settings, global Tauri bridge, and local asset directory (`../ui`). The main window and child webviews are created in Rust using Tauri's `unstable` multiwebview feature.
- `src-tauri/permissions/*.toml`: custom IPC permission definitions. `src-tauri/capabilities/`: `default.json` grants remote media/Discord permissions to the `content` webview and `https://anivox.fun/*`; `toolbar.json` grants local browser controls only to `toolbar`; other files are per local webview.
- `.cargo/config.toml`: selects `rust-lld.exe` for Windows MSVC linking.
- `run.bat`: adds Bun's user installation to PATH and starts development.
- `CHANGELOG.md`: release history; its sections feed GitHub Releases.
- `.github/workflows/build.yml`: CI/CD for cross-platform builds (Windows and Linux) on push to `main` and release assets on tags.
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
cargo check --locked --workspace --manifest-path src-tauri/Cargo.toml  # CI uses --locked
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check           # CI fails on formatting
cargo test --workspace --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml mpv_tests           # Rust tests filtered by module/name
node --test tests/*.test.mjs                                        # all JS tests
node --test tests/settings.test.mjs                                 # one test file
node --test --test-name-pattern="Escape" tests/settings.test.mjs   # one test by name
```

`package.json` defines `dev`, `build`, `build:arch`, `install:arch`, and `tauri`. Windows builds produce only the NSIS `setup.exe` and MSI (`bundle.targets` in `tauri.conf.json`); on Linux `build:arch` runs `tauri build --no-bundle` and packs the binary into `.pkg.tar.zst` via `PKGBUILD` (no AppImage/deb/rpm, no portable exe). Builds require the platform's Tauri/Rust prerequisites. Windows linking requires the configured `rust-lld.exe`.

Windows integration checks run against a release build, with other Anivox instances closed:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tests/windows_taskbar_icon.ps1 -Exe src-tauri/target/release/anivox.exe
powershell -NoProfile -ExecutionPolicy Bypass -File tests/windows_single_instance.ps1 -Exe src-tauri/target/release/anivox.exe
```

The taskbar script launches a test instance and checks that both `ICON_SMALL` and `ICON_BIG` contain valid window icons; it stops only its own instance.

## Implementation conventions

- Keep `main.rs` minimal; `lib.rs` wires modules together, so put new feature logic in its own module under `src-tauri/src/` (commands are `pub`, shared items `pub(crate)`).
- Change `ui/` for toolbar design and controls; change `src-tauri/scripts/inject-common.js` for cross-platform MPV/Discord hooks, `player-fullscreen.js` for fullscreen behaviour, `common-styles.js` for shared CSS, `inject-windows.js` for Windows/VSR DirectComposition styles, and `inject-linux.js` for WebKitGTK-specific engine/buffer optimizations.
- **Toolbar markup is duplicated:** add or change a button in both `ui/index.html` (Windows) and `ui/toolbar-fragment.html` (Linux). Toolbar changes must work in both modes. `tests/toolbar_parity.test.mjs` fails when the two `<nav>` blocks differ.
- Register new IPC commands in `tauri::generate_handler!`, define their permissions under `src-tauri/permissions/`, and enable them in the right capability file. Keep remote access scoped to the intended origin.
- Retain platform-specific `#[cfg(target_os = ...)]` branches when changing process execution.
- Shikimori has no Anivox/MPV access: its origin-scoped capability only permits closing its own overlay on Escape. Navigation controls belong to the local overlay and application toolbar. Keep Shikimori's HTTPS navigation and engine-managed login popups enabled for social sign-in; external login providers receive no IPC permissions, so do not broaden the origin-scoped capability to enable OAuth.
- Shikimori opens lazily in `shikimori` with local controls/backdrop in `shikimori-overlay`. Closing destroys both views; reopening creates a new Shikimori page: the same title if `content` is on `anivox.fun/anime/<id>-slug` (the numeric id matches Shikimori's `/animes/<id>`), otherwise the homepage. Windows positions child webviews; Linux reparents existing WebKit widgets into a native `GtkOverlay` while keeping the Anivox view alive. Keep browser arguments consistent across Windows webviews sharing a WebView2 profile.
- Local panels (`settings`, `update-review`, `connection`) are transparent child overlay webviews managed by `local_overlay.rs`. Only one opens at a time, anchored to its toolbar button. Close the child webview, NEVER its parent window; keep local origin/label guards (`ensure_local_window`). `ui/local-overlay.js` handles positioning, outside click and focus trapping.
- MPV launch uses the executable selected in local settings, otherwise `mpv` on PATH. Settings are saved atomically under Tauri `app_config_dir`; per-launch logs go to `app_local_data_dir/logs`. Preserve metadata, headers, start time and subtitles. Only the local settings webview may choose or save an executable path. Discord uses the active MPV session while its local JSON IPC is ready; embedded snapshots are retained for fallback. Optional `mirrorEmbedded` follows real MPV state on the same episode with mute/minimum quality; never synthesize viewing events. Resume history is local, atomic and keyed by `anime:episode:translation`.
- WebView2 browser flags and injected video styling support the project's NVIDIA video-processing integration; preserve their intent during UI or player changes.
- Update installation belongs only to local `update-review` and requires the reviewed version. Remote content and the toolbar may open that window but cannot install directly. Release notes from CHANGELOG must feed both GitHub Release and `latest.json`.
- There is no VPN/WireGuard module (removed in 0.3.4), no `proxy.rs`, no embedded wireproxy and no root `src/` template.
- User-facing docs (README, CHANGELOG) are written in Russian.

## Verification

- For Rust changes, run `cargo check --locked` and `cargo fmt -- --check` (CI fails on formatting), and distinguish existing formatting differences from changes introduced by the task.
- JS tests use Node's built-in runner: `ui/*.js` are loaded into a `node:vm` sandbox with a mocked `window.__TAURI__` (`tests/local_window_harness.mjs`, `tests/toolbar.test.mjs`). The `invoke` mock is keyed by IPC command name, so renaming a command means updating the tests. Run `node --test tests/*.test.mjs`.
- For injection changes, manually verify with `bun run dev`: navigation controls, remote IPC, Discord updates, MPV handoff, connection panel, and fullscreen/video layout as applicable. Verify Shikimori opens above Anivox, unloads via the button/Escape/backdrop, starts at its homepage on reopening, and follows window resizing without reloading Anivox.
- Discord, MPV and the remote site are external runtime dependencies; report which integrations were actually exercised.
- Documentation-only edits do not require launching or building the application.
- For Windows taskbar icon changes, build a release executable and run the taskbar script above.

## Architecture notes

- **Content webview init script** (assembled near the end of `lib.rs`) is, in order: a token-guarded `restore-paused.js` (blocks autoplay on a restored page until a trusted user gesture), then platform script + `mpv-session.js` + `inject-common.js` + `player-fullscreen.js` + `common-styles.js`, then `site-lifecycle.js` (reports top-level document/SPA load state to the native watchdog, never media buffering) and `segment-monitor.js`. `shikimori.js` is injected only into the Shikimori webview.
- **On Linux the toolbar is not a separate webview:** `ui/toolbar-fragment.html`, `toolbar-overlay.css` and `toolbar.js` are embedded via `include_str!` and injected into the remote page.
- **Closing the main window hides it to tray**; the app exits only via the tray menu "Exit". A second launch focuses the existing instance (single-instance).

## Release

The tag `vX.Y.Z` takes its notes from the `## [X.Y.Z]` section of `CHANGELOG.md` via `scripts/generate-updater-manifest.mjs`. A missing or empty section stops the release. Keep `package.json`, `PKGBUILD`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json` the version label in `ui/index.html` / `ui/toolbar-fragment.html`, `linux-gtk/Cargo.toml`, `linux-gtk/Cargo.lock`, `linux-gtk/arch/PKGBUILD`, `windows-winui/Anivox.WinUI.csproj`, `windows-winui/native/Cargo.toml` and `windows-winui/native/Cargo.lock` in sync. `tests/version_sync.test.mjs` fails when they diverge. The Windows CI job runs on a self-hosted runner (`actions-runner/` is git-ignored) and builds a separate `com.yasin.anivox.ci-native` identifier for native tests.

## Архитектура

### Назначение и сборка

Tauri v2, Rust 2021, Windows/WebView2 и Linux/WebKitGTK 4.1. Идентификатор приложения — `com.yasin.anivox`. Целевой сайт — `https://anivox.fun/`. Пакетный менеджер JavaScript — Bun. Команды сборки и тестов — выше и в README.

`src-tauri/src/main.rs` вызывает `anivox_lib::run()` и сохраняет подавление консоли Windows release. Backend разложен по модулям `src-tauri/src/` (`lib.rs` собирает приложение); `frontendDist` в `tauri.conf.json` указывает на `../ui`.

### Окна и внедряемый код

- Windows: нативное окно `main` с child webviews `toolbar` (локальная панель, `ui/index.html`) и `content` (Anivox).
- Linux: окно/webview `content`; панель из `ui/toolbar-fragment.html`, `toolbar-overlay.css` и `toolbar.js` внедряется в удалённую страницу через Shadow DOM.
- Скрипты `inject-common.js`, `inject-windows.js`, `inject-linux.js` собираются через `include_str!`/`concat!`. Windows сохраняет флаги WebView2/NVIDIA; Linux — текущие GTK/WebKit обходы проблем движка.
- Shikimori создаётся лениво в `shikimori` с локальным `shikimori-overlay`. Закрытие уничтожает оба webview; повторное открытие загружает домашнюю страницу Shikimori.
- Локальные панели `settings`, `update-review` и `connection` — child webviews внутри главного окна (`local_overlay.rs`), а не отдельные окна. Одновременно открыта одна панель, привязанная к своей кнопке; повторное нажатие закрывает её. Закрывается только панель, никогда родительское окно. Панель `settings` на Windows создаётся заранее (`prewarm`) и только скрывается/показывается; на Linux панели создаются и закрываются заново.
- Все Windows webviews используют совместимые browser arguments для общего WebView2-профиля.

Single Instance плагин регистрируется первым. Второй процесс показывает, восстанавливает и фокусирует существующее основное окно. Крестик основного окна скрывает его в трей, «Выход» завершает приложение.

### Проверка соединения

Кнопка с иконкой Wi‑Fi открывает локальную панель `connection` (`ui/connection.html`/`connection.js`). Команда `open_connection` доступна только панели приложения (`ensure_toolbar_access`).

- `src-tauri/scripts/segment-monitor.js` внедряется в `content` (только `https://anivox.fun`, только верхний документ) и пассивно наблюдает за XHR/fetch видео-сегментов (`.ts`, `.m4s`, `.mp4`...), ничего не меняя в запросах. Скорость считается по скользящему окну 6 с, незавершённые запросы учитываются сразу, отмена загрузки плеером не считается ошибкой. Раз в секунду скрипт вызывает `report_segment_stats`.
- `report_segment_stats` принимает данные только от webview `content` (право `allow-segment-stats-report` в `capabilities/default.json`); `get_segment_stats` читает только локальная панель `connection` (`capabilities/connection.json`). Состояние хранится в `SegmentStatsState`.
- Панель опрашивает статистику каждую секунду, отдельно измеряет скорость интернета скачиванием файла с `speed.cloudflare.com` и сравнивает значения. Скорость ниже `GOOD_MBPS` (10 Мбит/с) подсвечивается красным; данные сайта старше 4 с считаются отсутствующими.

### MPV и настройки

`open_in_mpv` читает `PlayerSettings` из `settings.json` в Tauri `app_config_dir`. Путь выбирается в локальной панели настроек через системный диалог; пустое значение означает `mpv` в PATH. Явный путь проверяется и не заменяется запасным EXE при ошибке.

Настройки сохраняются атомарно через временный файл в том же каталоге и замену назначения. Повреждённый файл при чтении не перезаписывается. Параллельные чтения/сохранения сериализуются.

Журналы MPV создаются в `app_local_data_dir/logs` отдельным файлом на запуск. Каталог установленного EXE и `F:\Mpv` не используются. Параметры передаются напрямую через `std::process::Command`: URL, заголовок, стартовая позиция, субтитры, HTTP-заголовки и параметры качества. После готовности MPV встроенное видео ставится на паузу либо зеркалит реальное воспроизведение без звука на минимальном качестве, если включён `mirrorEmbedded`. Наблюдаются `pause`, `time-pos`, `duration`, `speed`, `paused-for-cache` и `eof-reached`; локальный named pipe/Unix socket уникален для сессии. Прогресс сохраняется атомарно в `playback-history.json` по ключу `anime:episode:translation`. Anivox не завершает запущенный MPV при собственном выходе.

### Discord

`DiscordPresence` и `DiscordState` обслуживают встроенный плеер через `set_discord_rpc`. Инъекция собирает метаданные и состояние видео; backend обновляет Rich Presence с восстановлением соединения. Активная сессия внешнего MPV имеет приоритет над встроенным плеером. Метаданные замораживаются при handoff, а состояние и таймер берутся из MPV IPC. При завершении или потере IPC возвращается последнее состояние встроенного плеера. Перед строкой статуса добавляется рейтинг Shikimori («★ 9.05 · TV · 2019 · Серия 5 · MPV · Смотрит»): `ensure_shikimori_info` берёт данные из публичного `shikimori.io/api/animes/<id>` по id из `shikimori_url`, кэширует (сбой запоминается на 5 минут), `decorate_presence` подставляет строку при отправке в Discord и ничего не сохраняет в `embedded`/MPV-сессии. Пока открыт оверлей Shikimori и MPV не играет, вместо встроенного плеера показывается активность по типу страницы («Смотрит топ аниме», «Листает Shikimori»); страница определяется по URL webview (`shikimori_presence`, опрос раз в 2 с), без IPC от Shikimori, а имена профилей и запросы поиска в Discord не попадают.

### Обновления

`check_update` возвращает версию и описание. Панель вызывает `open_update_review`; локальная панель показывает changelog как текст. `install_update` доступен только `update-review`, принимает `expectedVersion` и повторно проверяет версию перед скачиванием. При расхождении возвращается `update_changed` с новым описанием, требующим нового подтверждения. Параллельная установка блокируется.

События `update-started`/`update-progress`/`update-finished`/`update-idle` управляют состоянием панели и окна обновления. Ошибки позволяют повторить попытку. Linux вне AppImage после подтверждения открывает страницу ручного обновления; остальные поддерживаемые сборки используют Tauri updater.

CI сначала извлекает notes для точной версии из CHANGELOG в `release-notes.md`, затем создаёт `latest.json` с тем же текстом и передаёт файл в GitHub Release. Пустая или отсутствующая секция — ошибка выпуска.

### Разрешения и проверки

Каждая новая команда регистрируется в `generate_handler!`, описывается в `permissions/*.toml` и включается в подходящие capabilities. Чтение/изменение настроек и выбор EXE доступны только локальному `settings`; установка — только локальному `update-review`. Дополнительно проверяются метка webview и локальный URL (`ensure_local_window`); навигация локальных панелей на удалённые сайты блокируется.

Anivox `content` имеет origin-scoped права для `https://anivox.fun/*`. На Linux ему разрешено открывать локальные панели настроек и обновления, но не менять путь EXE или устанавливать обновление напрямую. Shikimori может только закрыть свой overlay; внешние OAuth-провайдеры не получают IPC.

CI: JS-тесты и `cargo test` на Windows/Linux, затем сборка. После Windows-сборки последовательно проверяются `ICON_SMALL`/`ICON_BIG` и single-instance/восстановление окна. Выпуск зависит от успешного завершения обеих платформ.
