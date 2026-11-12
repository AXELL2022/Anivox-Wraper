# Anivox — минимальная сборка WinUI 3 (экспериментально)

Отдельная нативная оболочка для Windows на WinUI 3 (Windows App SDK 2.6) и WebView2, без Tauri.
Не входит в `cargo test --workspace` и в CI.

Что есть:

- окно с Mica и собственной панелью в заголовке: «Назад», «Вперёд», «Обновить», «Главная», индикатор загрузки;
- WebView2 открывает `https://anivox.fun/`, заголовок окна берётся со страницы;
- полноэкранный режим плеера переводит окно в fullscreen и прячет панель;
- ссылки `anivox.fun`/`anivox.me` из новых окон открываются в том же окне, прочие — в браузере по умолчанию,
  всплывающие окна входа через соцсети работают внутри WebView2;
- те же флаги WebView2 для NVIDIA RTX VSR/TrueHDR, что и в Tauri-сборке (`BROWSER_ARGS` в `src-tauri/src/lib.rs`);
- для RTX VSR WebView2 встроен как дочерний HWND (`CoreWebView2Controller`, windowed hosting, как в Tauri), а не
  XAML-контролом `WebView2`: тот рисует через композицию WinUI (visual hosting), и видео не попадает в оверлей
  DirectComposition. В страницу внедряется тот же `src-tauri/scripts/inject-windows.js` со сбросами CSS для оверлея;
- на Windows 11 такой WebView2 внутри WinUI не получает клики
  ([microsoft-ui-xaml#10826](https://github.com/microsoft/microsoft-ui-xaml/issues/10826)): окно Chromium
  `Intermediate D3D Window` с `WS_EX_TRANSPARENT` лежит поверх `Chrome_RenderWidgetHostHWND`. `WebViewHostFix.cs`
  раз в секунду снимает этот стиль и держит окно WebView2 выше окна ввода WinUI (`DesktopChildSiteBridge`);
- профиль WebView2 хранится в `%LOCALAPPDATA%\com.yasin.anivox.winui\WebView2` (отдельно от Tauri-версии).

Чего нет: MPV, Discord, Shikimori, трей, настройки, обновления, проверка соединения, внедряемые скрипты.

## Сборка

Нужен .NET 10 SDK (`winget install Microsoft.DotNet.SDK.10`).

```sh
dotnet build windows-winui/Anivox.WinUI.csproj -c Release
```

Результат: `windows-winui/bin/x64/Release/net10.0-windows10.0.19041.0/win-x64/anivox-winui.exe`.
Сборка unpackaged и self-contained (.NET и Windows App SDK лежат рядом с exe, ~230 МБ), отдельная установка
рантаймов не нужна. Для ARM64: `-p:Platform=ARM64`.
