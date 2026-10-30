---
name: release
description: Подготовка релиза Anivox - синхронизация версии во всех файлах, проверка секции CHANGELOG, прогон проверок перед тегом
disable-model-invocation: true
argument-hint: <X.Y.Z>
---

Подготовь релиз версии `$ARGUMENTS`. Тег и push делает только пользователь.

1. Версия должна совпасть во всех файлах: `package.json`, `PKGBUILD`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` (пакет `anivox`, обновлять через `cargo check`, не вручную), `src-tauri/tauri.conf.json`, метка версии в `ui/index.html` и `ui/toolbar-fragment.html`. Сначала найди текущую версию через Grep и покажи, что меняется.
2. В `CHANGELOG.md` должна быть непустая секция `## [X.Y.Z]` на русском. Если её нет, предложи текст по `git log` с прошлого тега и дождись одобрения.
3. Проверь заметки: `node scripts/generate-updater-manifest.mjs` (посмотри параметры в скрипте) или `node --test tests/updater_manifest.test.mjs`.
4. Запусти проверки: `cargo check --locked --manifest-path src-tauri/Cargo.toml`, `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`, `cargo test --manifest-path src-tauri/Cargo.toml`, `node --test tests/*.test.mjs`.
5. Отчитайся: что изменено, что прошло, что не проверено. Команды `git tag` и `git push` выведи для пользователя, сам не выполняй.
