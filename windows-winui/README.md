# Anivox — сборка WinUI 3 (экспериментально)

Отдельная нативная оболочка для Windows на WinUI 3 (C#, .NET 10, Windows App SDK 2.6) и WebView2, без Tauri.
Логика берётся из общего Rust-ядра `anivox-core` через тонкий C ABI (`native/`, `anivox_ffi.dll`), как в
GTK-версии: те же команды сайта, те же файлы настроек и истории. Не входит в `cargo test --workspace` и в CI.

Функции перенесены из Tauri/GTK-версий по этапам.

Готово:

- окно с Mica и собственной панелью в заголовке: «Назад», «Вперёд», «Обновить», «Главная», индикатор загрузки,
  версия приложения;
- кнопка MPV: передаёт текущую серию в MPV (`window.__anivoxLaunchMpv`) и показывает статус плеера от сайта
  (`report_player_status`) с его цветом; пояснения выводятся рядом и скрываются через 5 с;
- панель настроек (шестерёнка): страница при запуске, путь к MPV с выбором файла, «Продолжать воспроизведение на
  сайте при просмотре в MPV», очистка сохранённых позиций, папка журналов MPV. Настройки общие с Tauri-версией
  (`settings.json`); путь проверяется ядром. У сайта нет команд для этих действий. Панель — Flyout с
  `ShouldConstrainToRootBounds="False"`: так WinUI рисует её в отдельном окне поверх HWND сайта;
- внедряемые скрипты сайта в том же порядке, что в Tauri-сборке под Windows (`inject-windows.js`,
  `mpv-session.js`, `inject-common.js`, `player-fullscreen.js`, `common-styles.js`, `site-lifecycle.js`,
  `segment-monitor.js`), каждый только в верхнем документе `anivox.fun`/`anivox.me`;
- IPC-мост: `scripts/ipc-shim.js` даёт сайту `window.__TAURI__.core.invoke` поверх `chrome.webview.postMessage`;
  оболочка принимает сообщения только от `https://anivox.fun`/`https://anivox.me` и не отвечает другой странице.
  Команды те же, что в GTK-версии: `set_discord_rpc`, `open_in_mpv`, `get_mpv_state`, `get_playback_resume`,
  `report_player_status`, `content_fullscreen`, `report_segment_stats`, `get_site_document_generation`,
  `site_document_state`;
- сеанс в `%APPDATA%\com.yasin.anivox\session-winui.json`: позиция, размер, развёрнутость и последняя страница
  (без страниц входа и токенов, `session_page::trusted_page`); восстановленная страница не запускает видео до клика
  (`restore-paused.js`);
- один экземпляр: повторный запуск показывает и активирует уже открытое окно;
- значок в трее с меню «Открыть», «Сворачивать в трей», «Выход»; крестик прячет окно в трей, настройка общая с
  Tauri-версией (`tray.json`);
- полноэкранный режим плеера переводит окно в fullscreen и прячет панель;
- ссылки `anivox.fun`/`anivox.me` из новых окон открываются в том же окне, прочие — в браузере по умолчанию,
  всплывающие окна входа через соцсети работают внутри WebView2;
- те же флаги WebView2 для NVIDIA RTX VSR/TrueHDR, что и в Tauri-сборке (`BROWSER_ARGS` в `src-tauri/src/lib.rs`);
- для RTX VSR WebView2 встроен как дочерний HWND (`CoreWebView2Controller`, windowed hosting, как в Tauri), а не
  XAML-контролом `WebView2`: тот рисует через композицию WinUI (visual hosting), и видео не попадает в оверлей
  DirectComposition;
- на Windows 11 такой WebView2 внутри WinUI не получает клики
  ([microsoft-ui-xaml#10826](https://github.com/microsoft/microsoft-ui-xaml/issues/10826)): окно Chromium
  `Intermediate D3D Window` с `WS_EX_TRANSPARENT` лежит поверх `Chrome_RenderWidgetHostHWND`. `WebViewHostFix.cs`
  раз в секунду снимает этот стиль и держит окно WebView2 выше окна ввода WinUI (`DesktopChildSiteBridge`).

- оверлей Shikimori (кнопка «Shikimori»): открывается тот же тайтл, что на странице Anivox (`/anime/<id>` →
  `shikimori.io/animes/z<id>`), иначе главная Shikimori; панель с кнопками «Назад», «Вперёд», «Обновить»,
  «Закрыть». Закрывается кнопкой, Escape (в странице и в панели) и щелчком по фону; закрытие уничтожает WebView2,
  следующее открытие начинается заново. У Shikimori нет IPC Anivox: единственное сообщение закрывает оверлей и
  принимается только от `https://shikimori.io`. Навигация только по HTTPS, окна входа через соцсети — штатные
  окна WebView2. XAML нельзя нарисовать поверх HWND сайта, поэтому на время оверлея HWND сайта скрывается
  (страница не перезагружается), а фоном служит затемнённый снимок страницы (`CapturePreviewAsync`).

Discord работает через ядро так же, как в GTK-версии (`set_discord_rpc`, приоритет активной сессии MPV, рейтинг
Shikimori в статусе). Пока открыт Shikimori и MPV не играет, в Discord показывается только тип страницы
(«Читает об аниме на Shikimori», «Смотрит топ аниме»…); имена профилей и запросы поиска туда не попадают.

- проверка соединения (кнопка Wi‑Fi): сравнивает пассивную статистику видеосегментов (`segment-monitor.js`,
  `report_segment_stats`) с замером скорости через speed.cloudflare.com; вердикт «ваш интернет или видеосервер» —
  та же логика, что в `ui/connection.js` и GTK-версии (в ядре, `native/src/connection.rs`);
- экран восстановления: если сайт за 25 с не сообщил о готовности (`site-lifecycle.js`) или загрузка завершилась
  ошибкой, показывается экран с кнопками «Повторить загрузку», «На зеркало», «На главную» и «Продолжить ожидание».
  В отличие от GTK-версии он не показывается во время обычной загрузки (индикатор крутится в заголовке): для этого
  пришлось бы скрывать HWND сайта, а скрытые страницы Chromium притормаживает;
- обновления: через 3 с после запуска читается `latest.json` (тот же, что у Tauri updater). При новой версии в
  заголовке появляется кнопка с номером версии и changelog обычным текстом. Сборка WinUI не публикуется в релизах и
  не обновляется на месте: после повторной проверки версии открывается страница выпуска.

Функции Tauri/GTK-версий перенесены полностью. Отличия: экран восстановления (см. выше), оверлей Shikimori скрывает
HWND сайта и показывает его снимок.

Профиль WebView2 хранится в `%LOCALAPPDATA%\com.yasin.anivox.winui\WebView2` (отдельно от Tauri-версии);
настройки, история просмотра и журналы MPV — в общих с Tauri-версией каталогах `%APPDATA%\com.yasin.anivox` и
`%LOCALAPPDATA%\com.yasin.anivox\logs`.

## Сборка

Нужны .NET 10 SDK (`winget install Microsoft.DotNet.SDK.10`) и Rust с целью `x86_64-pc-windows-msvc`.

```sh
dotnet build windows-winui/Anivox.WinUI.csproj -c Release
```

Сборка сначала запускает `cargo build --locked --release` для `native/` и копирует `anivox_ffi.dll` рядом с exe.
Результат: `windows-winui/bin/x64/Release/net10.0-windows10.0.19041.0/win-x64/anivox-winui.exe`.
Сборка unpackaged и self-contained (.NET и Windows App SDK лежат рядом с exe, ~230 МБ), отдельная установка
рантаймов не нужна. Для ARM64: `-p:Platform=ARM64` (нужна цель Rust `aarch64-pc-windows-msvc`).

Тесты ядра:

```sh
cargo test --manifest-path windows-winui/native/Cargo.toml
```
