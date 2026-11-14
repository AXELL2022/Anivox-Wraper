using System;
using System.Diagnostics;
using System.IO;
using System.Text.Json.Nodes;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.Web.WebView2.Core;
using Windows.Graphics;

namespace Anivox.WinUI;

public sealed partial class MainWindow : Window
{
    private const string HomeUrl = "https://anivox.fun/";

    // Same flags as the former Tauri Windows build (BROWSER_ARGS): NVIDIA RTX VSR / TrueHDR
    // through DirectComposition video overlays and hardware decoding.
    private const string BrowserArgs =
        "--use-angle=d3d11 --enable-features=NvidiaVpSuperResolution,NvidiaVpTrueHDR," +
        "DirectCompositionVideoOverlays,DirectCompositionLetterboxVideoOptimization," +
        "DirectCompositionScalableVideoProcessing,DirectCompositionUseNV12DecodeSwapChain," +
        "UseD3D11VideoProcessor,D3D11VideoDecoder,HardwareAcceleratedVideoDecode,AllowDcompOverlaysInBackbuffer " +
        "--disable-features=msEdgeVideoSuperResolution,msWebOOUI,msPdfOOUI,msSmartScreenProtection " +
        "--enable-nv12-dxgi-video --ignore-gpu-blocklist --enable-gpu-rasterization --force-high-performance-gpu";

    private readonly IntPtr _hwnd;
    private readonly AnivoxCore _core = new();
    private readonly JsonObject _session;
    private readonly DispatcherQueueTimer _sessionTimer;
    private readonly TrayIcon _tray;
    private CoreWebView2Environment? _env;
    private CoreWebView2Controller? _controller;
    private DispatcherQueueTimer? _hostFixTimer;
    private bool _fullScreen;
    private OverlappedPresenter? _windowed;
    private bool _exiting;

    private CoreWebView2? Core => _controller?.CoreWebView2;

    public MainWindow()
    {
        InitializeComponent();
        _hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);

        ExtendsContentIntoTitleBar = true;
        SetTitleBar(DragRegion);
        AppWindow.TitleBar.PreferredHeightOption = TitleBarHeightOption.Tall;
        var iconPath = Path.Combine(AppContext.BaseDirectory, "Assets", "icon.ico");
        AppWindow.SetIcon(iconPath);
        if (AppWindow.Presenter is OverlappedPresenter overlapped)
        {
            overlapped.PreferredMinimumWidth = 420;
            overlapped.PreferredMinimumHeight = 300;
        }

        _session = _core.LoadSession();
        RestoreGeometry();

        _sessionTimer = DispatcherQueue.CreateTimer();
        _sessionTimer.Interval = TimeSpan.FromMilliseconds(600);
        _sessionTimer.IsRepeating = false;
        _sessionTimer.Tick += (_, _) => FlushSession();
        InitPlayer();
        InitSettings();
        InitUpdates();

        _tray = new TrayIcon(iconPath, _core.MinimizeToTray);
        _tray.Toggle += () =>
        {
            if (AppWindow.IsVisible)
            {
                HideToTray();
            }
            else
            {
                ShowFromTray();
            }
        };
        _tray.Open += ShowFromTray;
        _tray.ToggleMinimize += ToggleMinimizeToTray;
        _tray.Quit += ExitApp;

        _core.UiEvent += value => DispatcherQueue.TryEnqueue(() => OnCoreEvent(value));

        ContentHost.SizeChanged += (_, _) => UpdateBounds();
        ContentHost.Loaded += (_, _) => ContentHost.XamlRoot.Changed += (_, _) => UpdateBounds();
        AppWindow.Changed += OnAppWindowChanged;
        AppWindow.Closing += OnClosing;
        Activated += (_, e) =>
        {
            if (e.WindowActivationState != WindowActivationState.Deactivated)
            {
                (_shikimori ?? _controller)?.MoveFocus(CoreWebView2MoveFocusReason.Programmatic);
            }
        };
        Closed += (_, _) => Shutdown();

        _ = InitWebViewAsync();
    }

    // ---- WebView2 ----------------------------------------------------------------------------

    private async Task InitWebViewAsync()
    {
        // Unpackaged apps default to "<exe>.WebView2" next to the exe, which is not writable
        // under Program Files, so keep the profile in LocalAppData.
        var userData = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "com.yasin.anivox.winui", "WebView2");
        var options = new CoreWebView2EnvironmentOptions { AdditionalBrowserArguments = BrowserArgs };
        var env = await CoreWebView2Environment.CreateWithOptionsAsync(null, userData, options);
        _env = env;

        // Windowed hosting: WebView2 gets its own child HWND and DirectComposition tree, exactly
        // like Tauri/wry. Visual hosting (the XAML WebView2 control) breaks RTX VSR.
        _controller = await env.CreateCoreWebView2ControllerAsync(
            CoreWebView2ControllerWindowReference.CreateFromWindowHandle((ulong)_hwnd));
        _controller.DefaultBackgroundColor = Microsoft.UI.ColorHelper.FromArgb(255, 15, 15, 15);
        UpdateBounds();

        WebViewHostFix.Apply(_hwnd);
        _hostFixTimer = DispatcherQueue.CreateTimer();
        _hostFixTimer.Interval = TimeSpan.FromSeconds(1);
        _hostFixTimer.Tick += (_, _) => WebViewHostFix.Apply(_hwnd);
        _hostFixTimer.Start();

        var core = _controller.CoreWebView2;
        core.Settings.IsStatusBarEnabled = false;
        core.HistoryChanged += (_, _) => UpdateNavButtons();
        core.SourceChanged += (_, _) => OnSourceChanged();
        core.NavigationStarting += (_, _) =>
        {
            Loading.IsActive = true;
            BeginLoadAttempt();
        };
        core.NavigationCompleted += (sender, args) =>
        {
            Loading.IsActive = false;
            UpdateNavButtons();
            WebViewHostFix.Apply(_hwnd);
            OnSiteNavigationCompleted(sender, args);
        };
        core.DocumentTitleChanged += (_, _) =>
            Title = string.IsNullOrWhiteSpace(core.DocumentTitle) ? "Anivox" : core.DocumentTitle;
        core.ContainsFullScreenElementChanged += (_, _) => SetFullScreen(core.ContainsFullScreenElement);
        core.NewWindowRequested += OnNewWindowRequested;
        core.WebMessageReceived += OnWebMessage;

        var startup = _session["startupUrl"]?.GetValue<string>() ?? HomeUrl;
        await core.AddScriptToExecuteOnDocumentCreatedAsync(AnivoxCore.ContentScript(startup));

        core.Navigate(startup);
        _controller.MoveFocus(CoreWebView2MoveFocusReason.Programmatic);
    }

    private void UpdateBounds()
    {
        if (_controller is not null)
        {
            PlaceOver(_controller, ContentHost);
        }
        if (_shikimori is not null)
        {
            PlaceOver(_shikimori, ShikimoriHost);
        }
    }

    /// <summary>Puts a WebView2 HWND exactly over a XAML placeholder element.</summary>
    private static void PlaceOver(CoreWebView2Controller controller, FrameworkElement host)
    {
        if (host.XamlRoot is null)
        {
            return;
        }

        // Controller bounds are in physical pixels relative to the window client area.
        var scale = host.XamlRoot.RasterizationScale;
        var offset = host.TransformToVisual(null).TransformPoint(new Windows.Foundation.Point(0, 0));
        controller.Bounds = new Windows.Foundation.Rect(
            Math.Round(offset.X * scale),
            Math.Round(offset.Y * scale),
            Math.Round(host.ActualWidth * scale),
            Math.Round(host.ActualHeight * scale));
    }

    private void OnSourceChanged()
    {
        var source = Core?.Source;
        _core.SetPage(source);
        if (source is not null && AnivoxCore.IsSiteUrl(source))
        {
            _session["lastPage"] = source;
            ScheduleSessionFlush();
        }
    }

    /// <summary>
    /// <c>window.__TAURI__.core.invoke</c> from <c>ipc-shim.js</c>: only the top document of a
    /// trusted site origin may call the core, and replies never go to another origin.
    /// </summary>
    private void OnWebMessage(CoreWebView2 sender, CoreWebView2WebMessageReceivedEventArgs e)
    {
        if (!AnivoxCore.IsSiteUrl(e.Source))
        {
            return;
        }

        JsonObject? message;
        try
        {
            message = JsonNode.Parse(e.WebMessageAsJson) as JsonObject;
        }
        catch (Exception)
        {
            return;
        }

        var id = message?["id"]?.GetValue<long>();
        var command = message?["cmd"]?.GetValue<string>();
        if (id is null || string.IsNullOrEmpty(command))
        {
            return;
        }

        var args = message!["args"]?.ToJsonString();
        _core.SetPage(sender.Source);
        Task.Run(() =>
        {
            CoreResult result;
            try
            {
                result = _core.Invoke(command, args);
            }
            catch (Exception ex)
            {
                result = new CoreResult(false, ex.Message);
            }

            DispatcherQueue.TryEnqueue(() => Reply(id.Value, result));
        });
    }

    private void Reply(long id, CoreResult result)
    {
        var core = Core;
        if (core is null || !AnivoxCore.IsSiteUrl(core.Source))
        {
            return;
        }

        var reply = new JsonObject { ["id"] = id, ["ok"] = result.Ok };
        if (result.Ok)
        {
            reply["value"] = JsonNode.Parse(result.Text);
        }
        else
        {
            reply["error"] = result.Text;
        }
        core.PostWebMessageAsJson(reply.ToJsonString());
    }

    private void OnCoreEvent(JsonObject value)
    {
        switch (value["type"]?.GetValue<string>())
        {
            case "fullscreen":
                SetFullScreen(value["on"]?.GetValue<bool>() == true);
                break;
            case "siteReady":
                FinishLoadAttempt(value["generation"]?.GetValue<ulong>() ?? 0);
                break;
            case "status":
                ShowPlayerStatus(
                    value["text"]?.GetValue<string>() ?? "",
                    value["color"]?.GetValue<string>(),
                    value["resetAfterMs"]?.GetValue<ulong>() ?? 0,
                    value["detail"]?.GetValue<string>());
                break;
        }
    }

    private void OnNewWindowRequested(object? sender, CoreWebView2NewWindowRequestedEventArgs e)
    {
        if (!Uri.TryCreate(e.Uri, UriKind.Absolute, out var uri))
        {
            return;
        }

        // Site links stay in this window; sized popups (social sign-in) open as WebView2 windows;
        // other user-initiated links go to the default browser.
        if (AnivoxCore.IsSiteUrl(uri.AbsoluteUri))
        {
            e.Handled = true;
            Core?.Navigate(uri.AbsoluteUri);
        }
        else if (e.IsUserInitiated && uri.Scheme is "http" or "https" && !e.WindowFeatures.HasSize)
        {
            e.Handled = true;
            Process.Start(new ProcessStartInfo(uri.AbsoluteUri) { UseShellExecute = true });
        }
    }

    private void SetFullScreen(bool fullScreen)
    {
        if (_fullScreen == fullScreen)
        {
            return;
        }
        _fullScreen = fullScreen;
        TitleBar.Visibility = fullScreen ? Visibility.Collapsed : Visibility.Visible;

        // Without this the switch plays the restore/maximize animations: the window visibly
        // shrinks to its normal size and grows back.
        WindowTransitions.Suspend(_hwnd, DispatcherQueue);
        if (fullScreen)
        {
            // Keep the windowed presenter: going back to it returns straight to its state
            // (maximized stays maximized) and keeps its minimum size. A new Default presenter
            // first restores the normal size and only then maximizes.
            _windowed = AppWindow.Presenter as OverlappedPresenter;
            AppWindow.SetPresenter(AppWindowPresenterKind.FullScreen);
        }
        else if (_windowed is not null)
        {
            AppWindow.SetPresenter(_windowed);
        }
        else
        {
            AppWindow.SetPresenter(AppWindowPresenterKind.Default);
        }
    }

    private void UpdateNavButtons()
    {
        BackButton.IsEnabled = Core?.CanGoBack == true;
        ForwardButton.IsEnabled = Core?.CanGoForward == true;
    }

    private void OnBack(object sender, RoutedEventArgs e) => Core?.GoBack();

    private void OnForward(object sender, RoutedEventArgs e) => Core?.GoForward();

    private void OnReload(object sender, RoutedEventArgs e) => Core?.Reload();

    private void OnHome(object sender, RoutedEventArgs e) => Core?.Navigate(HomeUrl);

    // ---- Session -----------------------------------------------------------------------------

    private void RestoreGeometry()
    {
        var width = _session["width"]?.GetValue<int>();
        var height = _session["height"]?.GetValue<int>();
        var x = _session["x"]?.GetValue<int>();
        var y = _session["y"]?.GetValue<int>();
        if (width is int w && height is int h)
        {
            var rect = new RectInt32(x ?? 0, y ?? 0, w, h);
            // Only reuse the position while it is still on a connected display.
            if (x is not null && y is not null && DisplayArea.GetFromRect(rect, DisplayAreaFallback.None) is not null)
            {
                AppWindow.MoveAndResize(rect);
            }
            else
            {
                AppWindow.Resize(new SizeInt32(w, h));
            }
        }
        else
        {
            AppWindow.Resize(new SizeInt32(1280, 800));
        }

        if (_session["maximized"]?.GetValue<bool>() == true && AppWindow.Presenter is OverlappedPresenter p)
        {
            p.Maximize();
        }
    }

    private void RecordGeometry()
    {
        if (_fullScreen || AppWindow.Presenter is not OverlappedPresenter presenter)
        {
            return;
        }

        switch (presenter.State)
        {
            case OverlappedPresenterState.Minimized:
                return;
            case OverlappedPresenterState.Maximized:
                // A maximized window keeps the previous normal size and position.
                _session["maximized"] = true;
                break;
            default:
                _session["maximized"] = false;
                _session["x"] = AppWindow.Position.X;
                _session["y"] = AppWindow.Position.Y;
                _session["width"] = AppWindow.Size.Width;
                _session["height"] = AppWindow.Size.Height;
                break;
        }
        ScheduleSessionFlush();
    }

    private void ScheduleSessionFlush()
    {
        _sessionTimer.Stop();
        _sessionTimer.Start();
    }

    private CoreResult FlushSession()
    {
        _sessionTimer.Stop();
        var data = _session.DeepClone().AsObject();
        data.Remove("startupUrl");
        var result = _core.SaveSession(data);
        if (!result.Ok)
        {
            Debug.WriteLine($"Не удалось сохранить сеанс: {result.Text}");
        }
        return result;
    }

    private void OnAppWindowChanged(AppWindow sender, AppWindowChangedEventArgs args)
    {
        if (args.DidPositionChange || args.DidSizeChange || args.DidPresenterChange)
        {
            RecordGeometry();
        }

        if (_controller is null)
        {
            return;
        }

        if (args.DidPositionChange)
        {
            _controller.NotifyParentWindowPositionChanged();
            _shikimori?.NotifyParentWindowPositionChanged();
        }

        if (args.DidPresenterChange || args.DidSizeChange || args.DidVisibilityChange)
        {
            UpdateWebViewVisibility();
        }
    }

    /// <summary>
    /// Stop rendering while minimized or hidden in the tray, like a background tab. While the
    /// Shikimori overlay is open the site HWND is hidden too (XAML cannot cover it).
    /// </summary>
    private void UpdateWebViewVisibility()
    {
        var minimized = AppWindow.Presenter is OverlappedPresenter p && p.State == OverlappedPresenterState.Minimized;
        var shown = AppWindow.IsVisible && !minimized;
        if (_controller is not null)
        {
            _controller.IsVisible = shown && _shikimori is null && !RecoveryVisible;
        }
        if (_shikimori is not null)
        {
            _shikimori.IsVisible = shown;
        }
    }

    // ---- Tray and lifetime -------------------------------------------------------------------

    /// <summary>The close button hides the window to the tray; "Выход" in the tray menu exits.</summary>
    private void OnClosing(AppWindow sender, AppWindowClosingEventArgs args)
    {
        if (!_exiting && _tray.IsShown && _tray.MinimizeToTray)
        {
            args.Cancel = true;
            HideToTray();
        }
    }

    private void HideToTray()
    {
        FlushSession();
        if (_fullScreen)
        {
            SetFullScreen(false);
        }
        AppWindow.Hide();
    }

    public void ShowFromTray()
    {
        AppWindow.Show();
        if (AppWindow.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Minimized } p)
        {
            p.Restore();
        }
        Activate();
        WinActivate.BringToFront(_hwnd);
    }

    private void ToggleMinimizeToTray()
    {
        var enabled = !_tray.MinimizeToTray;
        var result = _core.SaveMinimizeToTray(enabled);
        if (result.Ok)
        {
            _tray.MinimizeToTray = enabled;
        }
        else
        {
            Debug.WriteLine($"Не удалось сохранить настройку трея: {result.Text}");
        }
    }

    private void ExitApp()
    {
        _exiting = true;
        Close();
    }

    private void Shutdown()
    {
        FlushSession();
        _hostFixTimer?.Stop();
        _shikimoriTimer?.Stop();
        _shikimori?.Close();
        _controller?.Close();
        _tray.Dispose();
        // The core is not destroyed: a command may still run on the thread pool, and the
        // process ends right after. MPV keeps running on purpose (Anivox never ends it).
        Application.Current.Exit();
    }
}
