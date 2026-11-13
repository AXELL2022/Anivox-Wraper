---
name: release
description: Подготовка релиза Anivox - синхронизация версии во всех файлах, проверка секции CHANGELOG, прогон проверок перед тегом
disable-model-invocation: true
argument-hint: <X.Y.Z>
---

Подготовь релиз версии `$ARGUMENTS`. Тег и push делает только пользователь.

1. Версия должна совпасть во всех файлах из `tests/version_sync.test.mjs`: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `linux-gtk/Cargo.toml`, `linux-gtk/arch/PKGBUILD`, `windows-winui/Anivox.WinUI.csproj`, `windows-winui/native/Cargo.toml` и lock-файлы (`src-tauri/Cargo.lock`, `linux-gtk/Cargo.lock`, `windows-winui/native/Cargo.lock` обновлять через cargo, не вручную). Сначала найди текущую версию через Grep и покажи, что меняется.
2. В `CHANGELOG.md` должна быть непустая секция `## [X.Y.Z]` на русском. Если её нет, предложи текст по `git log` с прошлого тега и дождись одобрения.
3. Проверь заметки: `node scripts/generate-updater-manifest.mjs` (посмотри параметры в скрипте) или `node --test tests/updater_manifest.test.mjs`.
4. Запусти проверки из раздела «Проверки» в `README.md` (JS-тесты, тесты ядра, `anivox-ffi`, GTK в WSL, форматирование всех трёх манифестов) и `dotnet build windows-winui/Anivox.WinUI.csproj -c Release`. Выпуск публикует `.github/workflows/release.yml` по тегу; первый выпуск на WinUI требует ручной проверки перехода с Tauri (установщик с `/P /UPDATE /R` поверх установленной Tauri-версии).
5. Отчитайся: что изменено, что прошло, что не проверено. Команды `git tag` и `git push` выведи для пользователя, сам не выполняй.
