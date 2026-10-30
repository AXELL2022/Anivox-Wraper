# CLAUDE.md

Руководство для агентов (Claude Code и др.) по репозиторию. Единственный файл с инструкциями: он заменил `AGENTS.md`, `GEMINI.md` и `INIT.md`. Если код расходится с текстом, прав код (актуально для версии 0.4.0).

## Project overview

Anivox is a Tauri v2 desktop wrapper for `https://anivox.fun/`, with a Rust backend and JavaScript/CSS injected into the remote webview. Features include navigation controls, Discord Rich Presence, external MPV playback, a Shikimori overlay, update review/installation and a connection check panel. Windows and Linux have platform-specific implementations.

## Source map

- `src-tauri/src/lib.rs`: application setup, native window with separate `toolbar` and `content` webviews, Discord state, MPV launch, IPC commands and initialization script assembly via `concat!` and `include_str!`.
- `src-tauri/src/main.rs`: minimal entry point calling `anivox_lib::run()`; preserves the Windows release console suppression attribute.
- `src-tauri/src/` modules: `session.rs` (window geometry and last-page persistence; read its module doc for the required call order around window creation, tray hiding and exit), `site_recovery.rs` (native loading watchdog; a timeout is not proof of network failure), `tray_settings.rs` (close-to-tray state), `local_overlay.rs` (local dropdown panels). Rust tests live in `*_tests.rs` files next to `lib.rs` and in inline `#[cfg(test)]` modules.
- `src-tauri/scripts/`: remote webview injection scripts: `inject-common.js` (MPV media hooks, fullscreen, Discord), `inject-windows.js` (RTX VSR DirectComposition CSS), `inject-linux.js` (WebKitGTK buffer/pacer workarounds), `mpv-session.js` (same-episode embedded mirroring driven by native MPV snapshots; loaded before `inject-common.js` on all platforms), `restore-paused.js`, `site-lifecycle.js`, `segment-monitor.js`, and `shikimori.js` (Shikimori webview only).
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
cargo check --locked --manifest-path src-tauri/Cargo.toml          # CI uses --locked
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check           # CI fails on formatting
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml mpv_tests           # Rust tests filtered by module/name
node --test tests/*.test.mjs                                        # all JS tests
node --test tests/settings.test.mjs                                 # one test file
node --test --test-name-pattern="Escape" tests/settings.test.mjs   # one test by name
```

`package.json` defines `dev`, `build`, `build:arch`, `install:arch`, and `tauri`. Builds produce host-platform Tauri bundles and require the platform's Tauri/Rust prerequisites. Windows linking requires the configured `rust-lld.exe`.

Windows integration checks run against a release build, with other Anivox instances closed:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tests/windows_taskbar_icon.ps1 -Exe src-tauri/target/release/anivox.exe
powershell -NoProfile -ExecutionPolicy Bypass -File tests/windows_single_instance.ps1 -Exe src-tauri/target/release/anivox.exe
```

The taskbar script launches a test instance and checks that both `ICON_SMALL` and `ICON_BIG` contain valid window icons; it stops only its own instance.

## Implementation conventions

- Keep backend application logic in `lib.rs` and the entry point minimal.
- Change `ui/` for toolbar design and controls; change `src-tauri/scripts/inject-common.js` for cross-platform remote hooks, `inject-windows.js` for Windows/VSR DirectComposition styles, and `inject-linux.js` for WebKitGTK-specific engine/buffer optimizations.
- **Toolbar markup is duplicated:** add or change a button in both `ui/index.html` (Windows) and `ui/toolbar-fragment.html` (Linux). Toolbar changes must work in both modes.
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

- **Content webview init script** (assembled near the end of `lib.rs`) is, in order: a token-guarded `restore-paused.js` (blocks autoplay on a restored page until a trusted user gesture), then platform script + `mpv-session.js` + `inject-common.js`, then `site-lifecycle.js` (reports top-level document/SPA load state to the native watchdog, never media buffering) and `segment-monitor.js`. `shikimori.js` is injected only into the Shikimori webview.
- **On Linux the toolbar is not a separate webview:** `ui/toolbar-fragment.html`, `toolbar-overlay.css` and `toolbar.js` are embedded via `include_str!` and injected into the remote page.
- **Closing the main window hides it to tray**; the app exits only via the tray menu "Exit". A second launch focuses the existing instance (single-instance).

## Release

The tag `vX.Y.Z` takes its notes from the `## [X.Y.Z]` section of `CHANGELOG.md` via `scripts/generate-updater-manifest.mjs`. A missing or empty section stops the release. Keep `package.json`, `PKGBUILD`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json` and the version label in `ui/index.html` / `ui/toolbar-fragment.html` in sync. The Windows CI job runs on a self-hosted runner (`actions-runner/` is git-ignored) and builds a separate `com.yasin.anivox.ci-native` identifier for native tests.

## Архитектура

### Назначение и сборка

Tauri v2, Rust 2021, Windows/WebView2 и Linux/WebKitGTK 4.1. Идентификатор приложения — `com.yasin.anivox`. Целевой сайт — `https://anivox.fun/`. Пакетный менеджер JavaScript — Bun. Команды сборки и тестов — выше и в README.

`src-tauri/src/main.rs` вызывает `anivox_lib::run()` и сохраняет подавление консоли Windows release. Backend остаётся в `src-tauri/src/lib.rs`; `frontendDist` в `tauri.conf.json` указывает на `../ui`.

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
