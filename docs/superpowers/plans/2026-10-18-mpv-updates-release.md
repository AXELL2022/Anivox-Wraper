# MPV Settings and Release Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Настраиваемый MPV и журналы Anivox, просмотр changelog перед установкой, проверки Rust/Windows в CI и удаление устаревших файлов.

**Architecture:** Логика Rust остаётся в `src-tauri/src/lib.rs`. Локальные окна `settings` и `update-review` из `ui/` имеют отдельные capabilities. Описание релиза извлекается один раз из CHANGELOG и используется для GitHub и updater manifest.

**Tech Stack:** Tauri v2, Rust 2021, serde_json, официальный tauri-plugin-dialog v2, JavaScript, Node test runner, PowerShell и GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-18-mpv-updates-release-design.md`

## Global Constraints

- Интеграция MPV с Discord отложена; существующее отслеживание встроенного плеера не менять.
- Версия остаётся `0.2.4`; публичный релиз не создавать.
- Сохранить текущие незакоммиченные изменения single-instance, значков и AGENTS.md.
- Новые IPC регистрировать и ограничивать предусмотренными webview; Shikimori и OAuth не получают новых прав.
- Платформенные ветви VPN, WebView2/NVIDIA и WebKitGTK сохранить.
- Настройки: `settings.json` в Tauri app_config_dir; журналы: `logs` в app_local_data_dir.
- Пустой путь MPV означает `mpv` из PATH, явный путь не имеет неявного fallback.

## Review Focus

1. Пути с пробелами и кириллицей передаются одним аргументом, без оболочки; покрыть тестами задачи 1.
2. Ошибочный settings.json и невозможность записи не уничтожают прежние данные; покрыть задачей 1.
3. Закрытие дополнительного окна не закрывает основное и не отключает VPN; покрыть задачей 2 и Windows-проверкой.
4. Новая версия между просмотром и установкой требует нового подтверждения; покрыть задачей 3.
5. Пустая секция changelog, отсутствующая версия и переносы CRLF не порождают ложный updater manifest; покрыть задачей 3.

## Task 1: Настройки, аргументы MPV и корректная передача видео

**Files:** modify `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/scripts/inject-common.js`; create `src-tauri/src/mpv_tests.rs`; modify `tests/mpv_stream.test.mjs`.

**Interfaces:**
- `PlayerSettings { mpv_path: Option<String> }`, serde camelCase, default means PATH.
- `load_player_settings(path: &Path) -> Result<PlayerSettings, String>`: absent file returns default; invalid JSON returns an error.
- `save_player_settings(path: &Path, settings: &PlayerSettings) -> Result<(), String>`: validate, write a sibling temporary file, sync and replace; preserve old file on failure. Serialize concurrent writes with a managed mutex.
- `validate_mpv_path(value: Option<String>) -> Result<Option<String>, String>`: trim, empty to None, reject relative or missing/non-file paths.
- `build_mpv_command(settings: &PlayerSettings, log_path: &Path, url: String, title: Option<String>, start_time: Option<f64>, sub_url: Option<String>, token: Option<String>) -> Command`.
- `open_in_mpv(app: AppHandle, ...) -> Result<(), String>` keeps existing frontend argument names; reads settings, creates logs directory, chooses unique `mpv-<time>-<counter>.log`, then spawns the command.

- [ ] Add Rust tests for missing/valid/corrupt settings; blank/reset, relative/missing/directory paths; round-trip Unicode path; failed replacement preserves old bytes. Use uniquely named temporary directories and clean them up.
- [ ] Add command tests asserting chosen program, one argument per path, preserved metadata/headers/start/subtitle flags, caller-provided log path, absence of disk-F defaults and forced working directory.
- [ ] Add JS tests with deferred/rejected MPV invoke: video stays playing until launch succeeds; failure does not pause; startTime still captures the original position.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml mpv_tests` and `node --test tests/mpv_stream.test.mjs`; confirm failures identify missing behavior.
- [ ] Implement these interfaces in lib.rs, register mpv_tests under cfg(test), and move `video.pause()` after successful MPV invocation. Do not alter Discord hooks.
- [ ] Run both test commands again and `cargo check --manifest-path src-tauri/Cargo.toml`; expect all targeted tests and check to pass.
- [ ] Commit only task-owned changes, preserving all pre-existing hunks. If a file mixes pre-existing and task changes, stage only the task's patch; never include the unrelated baseline.

## Task 2: Локальное окно настроек

**Files:** modify `src-tauri/src/lib.rs`, Cargo files, `ui/index.html`, `ui/toolbar-fragment.html`, `ui/toolbar.js`, `ui/toolbar.css`; create `ui/settings.html`, `ui/settings.js`, `ui/local-window.css`, `src-tauri/permissions/settings.toml`, `src-tauri/capabilities/settings.json`, `tests/settings.test.mjs`; modify toolbar/default capabilities and `tests/toolbar.test.mjs`.

**Interfaces:**
- `open_settings(webview: Webview, app: AppHandle) -> Result<(), String>`: toolbar access check, open/focus window label `settings` with local settings.html, BROWSER_ARGS consistent with other Windows webviews.
- `get_player_settings(webview: Webview, app: AppHandle) -> Result<PlayerSettingsInfo, String>`, info `{ mpvPath, logDirectory }`.
- `pick_mpv_executable(webview: Webview, app: AppHandle) -> Result<Option<String>, String>`: official dialog Rust API in an async callback or blocking worker; cancellation returns None, no persistence.
- `save_player_settings_command(webview: Webview, app: AppHandle, mpv_path: Option<String>) -> Result<PlayerSettingsInfo, String>` uses Task 1 helpers.
- get/pick/save require `settings` webview and a local app URL. Capabilities grant only the corresponding custom commands; no direct general filesystem or shell permissions.
- `is_primary_window(label: &str) -> bool`: true only for `main` and `content`. Apply main close-to-tray, resize layout and destruction/VPN exit handling only to these windows.

- [ ] Add JS behavior tests: toolbar opens settings once per click, errors restore controls; settings loads Unicode path, picker cancel preserves field, reset clears field, save persists only on success and displays errors without losing input.
- [ ] Add Rust tests for primary-window classification and caller-label checks; assert settings/update-review are secondary and content/toolbar cannot call settings mutation helpers.
- [ ] Run new tests to confirm expected failures.
- [ ] Add tauri-plugin-dialog `"2"`, register after single-instance. Implement the commands and singleton local window. Keep close handling scoped to primary windows; native close and Escape may destroy settings normally.
- [ ] Build settings UI with labelled input, Browse/Save/Reset buttons, visible logs path and status text. Button placement in both toolbars must fit the minimum window width without losing essential controls.
- [ ] Register new commands, permissions and capabilities; inspect that no remote origin gets executable-path mutation or file selection permission.
- [ ] Run settings/toolbar JS tests, Rust tests, cargo check. Open settings on Windows, resize and close it; confirm Anivox remains alive and the remote view is not reloaded.
- [ ] Commit only task-owned changes.

## Task 3: Общие release notes и просмотр перед установкой

**Files:** modify `scripts/generate-updater-manifest.mjs`, `.github/workflows/build.yml`, `ui/toolbar.js`, `src-tauri/src/lib.rs`, update permissions/capabilities, `tests/updater_manifest.test.mjs`, `tests/toolbar.test.mjs`, existing Rust outcome test; create `ui/update-review.html`, `ui/update-review.js`, `src-tauri/capabilities/update-review.json`, `tests/update_review.test.mjs`.

**Interfaces:**
- `extractReleaseNotes(changelog: string, tag: string): string` exported by manifest script. Select exact version heading `## [version]` until next level-2 heading; normalize CRLF, remove final horizontal separator; throw for missing/empty section.
- `generateManifest({ inputDir, outputDir, tag, repo, notesPath })`: read shared UTF-8 release-notes.md and set manifest.notes to that text; fail on missing/blank notes.
- Script CLI mode `--extract-notes`: reads CHANGELOG.md plus RELEASE_TAG, writes release-notes.md. CI runs this mode before normal manifest generation; GitHub body_path uses the same file.
- `open_update_review(webview: Webview, app: AppHandle) -> Result<(), String>`: toolbar check and singleton local window `update-review`, consistent Windows browser flags.
- `install_update(webview: Webview, app: AppHandle, expected_version: String) -> Result<UpdateInstallOutcome, String>`: local update-review only; check current version before download or Linux manual-release navigation.
- Keep `no_update` and `release_page` outcomes; add `UpdateChanged { version: String, body: Option<String> }` (serde snake_case) when the current version differs.
- Review UI uses `check_update` to get current info; calls install_update only after Install click with displayed version. A changed outcome renders new info and re-enables confirmation. Events continue to update toolbar/review progress.

- [ ] Add manifest tests for exact matching among multiple versions, CRLF, missing/empty sections and exact note reuse in latest.json; use temporary paths, never create release artifacts in the repository root.
- [ ] Replace toolbar's install-on-click tests with open-review tests; assert clicking badge never invokes install_update and errors allow retry.
- [ ] Add review tests: loading/missing notes fallback, textContent rendering of HTML-like notes, Later/Escape never install, double click starts once, failure re-enables action, no_update and release_page messages, UpdateChanged requires a second confirmation with new version.
- [ ] Add Rust tests for version confirmation comparison and serialization of UpdateChanged; preserve existing outcome serialization checks.
- [ ] Run targeted tests; confirm failures before implementation.
- [ ] Implement extraction and order CI extraction before manifest generation. Implement local review window and capabilities using Task 2 secondary-window handling and shared local-window.css.
- [ ] Restrict install_update at runtime and in capabilities, removing remote/toolbar install grants. Keep check_update and open-review grants only where required. Preserve download progress and restart behavior.
- [ ] Run manifest/review/toolbar tests and full Rust suite; expect all pass.
- [ ] Commit only task-owned changes.

## Task 4: Проверки CI, очистка и актуальная документация

**Files:** modify `.github/workflows/build.yml`, README.md, INIT.md, CHANGELOG.md, AGENTS.md; delete tracked root src files and `src-tauri/binaries/wireproxy.exe`. Retain existing Windows test scripts and include them in the final deliverable.

**Interfaces:** no new product IPC. Existing Windows scripts accept `-Exe src-tauri/target/release/anivox.exe` and throw on failure.

- [ ] Add Rust tests step to both matrix platforms after dependencies, before build. Add sequential Windows icon and single-instance checks after successful release build and before artifact upload; release remains dependent on successful matrix build.
- [ ] Search tracked references using rg and `git ls-files`; verify frontendDist is ../ui and there are no active wireproxy/includes/build references. Resolve absolute deletion targets and confirm both remain inside D:\Anivox-Wraper before removing them with PowerShell LiteralPath.
- [ ] Remove the tracked template files and unused binary without rewriting history or removing backup_linux. Remove obsolete ignore/source-map statements for the deleted template where applicable.
- [ ] Rewrite README and INIT with actual Windows/Linux architecture, Bun/Rust commands, system WireGuard behavior and permissions, MPV selection/PATH/logs, built-in-player-only Discord and updater flow. Preserve historical CHANGELOG entries; add Unreleased notes for these changes. Update relevant AGENTS conventions, retaining prior icon guidance.
- [ ] Run `node --test tests/*.test.mjs`, `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo check --manifest-path src-tauri/Cargo.toml`, and `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`. Distinguish existing formatting differences recorded before this task from new ones.
- [ ] Build `cargo build --release --manifest-path src-tauri/Cargo.toml`. Run both Windows scripts sequentially via PowerShell. Confirm they stop only their test instances.
- [ ] Inspect actual settings and update-review windows, minimum-width toolbar and closing/reopening behavior. If UI automation is unavailable, report that explicitly instead of claiming visual QA.
- [ ] Exercise a locally available real MPV with a harmless local media sample using chosen path and PATH modes; verify log creation. If MPV is unavailable, mark real playback unverified and rely on command/launch tests.
- [ ] Inspect final diff, confirm no active hardcoded F:\Mpv, no unintended Discord changes, no template source or wireproxy binary in tracking; report local Windows results separately from unrun Linux/GitHub Actions.
- [ ] Commit cleanup/CI/docs task-owned changes. Present the new EXE path and concise verification results; do not publish or push a release.
