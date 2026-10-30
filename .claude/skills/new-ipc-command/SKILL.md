---
name: new-ipc-command
description: Добавление новой IPC-команды Tauri в Anivox со всеми обязательными местами (handler, permissions, capabilities, guard, тесты)
disable-model-invocation: true
argument-hint: <имя_команды> <какой webview её вызывает>
---

Добавь IPC-команду `$ARGUMENTS`. Пропуск любого шага ломает команду молча или открывает лишний доступ.

1. Функция `#[tauri::command]` в `src-tauri/src/lib.rs` (или подходящем модуле). Для локальных панелей добавь `ensure_local_window`/`ensure_toolbar_access`; для `content` проверяй метку webview.
2. Зарегистрируй в `tauri::generate_handler!`.
3. Опиши право в `src-tauri/permissions/*.toml`.
4. Включи право ровно в тот capability-файл (`src-tauri/capabilities/`), который нужен: `default.json` (content и `https://anivox.fun/*`), `toolbar.json`, либо файл конкретной локальной панели. Не расширяй права Shikimori и внешних OAuth-провайдеров.
5. Обнови моки `invoke` в `tests/*.test.mjs` (ключ - имя команды) и добавь тест на отказ чужому webview.
6. Проверь: `cargo check --locked --manifest-path src-tauri/Cargo.toml`, `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`, `node --test tests/*.test.mjs`.
