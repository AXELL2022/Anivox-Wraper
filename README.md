# Anivox Desktop

Настольная оболочка для [anivox.fun](https://anivox.fun/) на Tauri v2 для Windows и Linux.

## Возможности

- Независимая панель навигации: назад, вперёд, обновление страницы.
- Один экземпляр приложения: повторный запуск показывает существующее окно, включая скрытое в трей.
- Системный WireGuard VPN с переключателем и отображением ошибок.
- Открытие текущего видео в MPV с передачей позиции, заголовка, субтитров и HTTP-заголовков.
- Discord Rich Presence для встроенного плеера: аниме, серия, постер и оставшееся время.
- Shikimori поверх сайта; закрытие не перезагружает Anivox.
- Просмотр изменений перед установкой обновления, прогресс загрузки и перезапуск.

## Установка и зависимости

Готовые сборки находятся в [GitHub Releases](https://github.com/AXELL2022/Anivox-Wraper/releases).

**Windows:** требуется WebView2 Runtime. Для VPN отдельно установите WireGuard, для внешнего воспроизведения — MPV.

**Linux:** требуется GTK 3 и WebKitGTK 4.1. Для VPN используются NetworkManager (`nmcli`) либо `wg-quick` из wireguard-tools. MPV устанавливается отдельно. Доступны AppImage, deb и пакет Arch Linux из процесса сборки.

## MPV и журналы

Нажмите ⚙ на панели. Выберите исполняемый файл MPV через «Обзор…» или укажите абсолютный путь, затем нажмите «Сохранить». Пустой путь означает поиск `mpv` в PATH. Сброс пути применяется после сохранения. Настройка действует при следующем открытии видео; существующий MPV не закрывается.

Настройки хранятся в `settings.json` в каталоге конфигурации Anivox. Журналы создаются отдельно для каждого запуска в `logs` внутри локального каталога данных приложения. Полный путь к журналам отображается в настройках.

| Система | Настройки | Журналы |
| --- | --- | --- |
| Windows | `%APPDATA%\com.yasin.anivox\settings.json` | `%LOCALAPPDATA%\com.yasin.anivox\logs\` |
| Linux | `$XDG_CONFIG_HOME/com.yasin.anivox/settings.json` | `$XDG_DATA_HOME/com.yasin.anivox/logs/` |

На Linux при незаданных XDG-переменных используются `~/.config` и `~/.local/share` соответственно.

Внешний плеер запускается кнопкой MPV или клавишей M / Ь. При ошибке запуска встроенное видео продолжает воспроизводиться. Discord пока отслеживает встроенный плеер; состояние внешнего MPV в карточке не отображается.

## VPN

Переключатель управляет **системным** туннелем WireGuard `ANIVOX-UA-48`, влияющим на маршруты и DNS компьютера. Встроенного wireproxy или SOCKS-прокси нет.

На Windows приложение использует WireGuard tunnel service. При отказе в доступе явное переключение запрашивает UAC только для операции WireGuard; само приложение не повышает права. Отмена UAC отображается в панели. На Linux используется `nmcli`, затем при необходимости `wg-quick`.

При запуске и штатном завершении приложение пытается отключить этот туннель. Автоматическая очистка не запрашивает UAC и может не сработать без достаточных прав. Закрытие основного окна скрывает его в трей; для завершения выберите «Выход» в меню трея.

## Разработка

Нужны Bun, актуальный stable Rust и системные зависимости Tauri v2. На Windows также нужны MSVC Build Tools и доступный `rust-lld.exe`, выбранный в `.cargo/config.toml`. Linux-зависимости перечислены в `.github/workflows/build.yml`.

```sh
bun install --frozen-lockfile
bun run dev
bun run build
bun run build:arch
bun run install:arch
bun run tauri info
```

Сборки находятся в `src-tauri/target/release/`; установочные пакеты — в подкаталогах `bundle/`.

## Проверки

```sh
node --test tests/*.test.mjs
cargo check --locked --manifest-path src-tauri/Cargo.toml
cargo test --locked --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

После release-сборки на Windows, закрыв остальные экземпляры Anivox:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tests/windows_taskbar_icon.ps1 -Exe src-tauri/target/release/anivox.exe
powershell -NoProfile -ExecutionPolicy Bypass -File tests/windows_single_instance.ps1 -Exe src-tauri/target/release/anivox.exe
```

Скрипты завершают только свои тестовые экземпляры. Проверки Discord, WireGuard, MPV и удалённого сайта требуют соответствующих внешних программ и доступа к сервисам.

## Обновления и выпуск

Значок обновления открывает окно с изменениями. «Позже» ничего не устанавливает; «Установить» повторно проверяет выбранную версию. Если вышла другая версия, приложение покажет её описание и запросит новое подтверждение. Для Linux вне AppImage открывается страница ручной установки.

CI запускает JS- и Rust-тесты, проверяет форматирование и компиляцию на Windows/Linux. Windows использует runner с метками `self-hosted, Windows, X64` и проверяет значки и single-instance; Linux собирается на GitHub-hosted Ubuntu. Каталог `actions-runner/` исключён из Git. Для тега `vX.Y.Z` описание берётся из секции `## [X.Y.Z]` в `CHANGELOG.md`. Один `release-notes.md` используется и для GitHub-релиза, и для поля `notes` в `latest.json`. Отсутствующая или пустая секция останавливает выпуск.

## Исходники

- `src-tauri/src/lib.rs` — окна, IPC, VPN, Discord, MPV, настройки и обновления.
- `src-tauri/scripts/` — общие и платформенные инъекции удалённого webview.
- `ui/` — локальная панель, Shikimori, настройки и описание обновления.
- `tests/` — JavaScript и Windows-интеграционные проверки; Rust-тесты расположены рядом с `lib.rs`.
- `scripts/generate-updater-manifest.mjs` — извлечение changelog и создание манифеста.

Подробности архитектуры — в `INIT.md`, правила разработки — в `AGENTS.md`, история — в `CHANGELOG.md`.
