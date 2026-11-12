using System;
using System.Diagnostics;
using System.IO;
using System.Threading.Tasks;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.Web.WebView2.Core;

namespace Anivox.WinUI;

public sealed partial class MainWindow : Window
{
    private static readonly Uri Home = new("https://anivox.fun/");

    // Same flags as the Tauri build (src-tauri/src/lib.rs BROWSER_ARGS): NVIDIA RTX VSR / TrueHDR
    // through DirectComposition video overlays and hardware decoding.
    private const string BrowserArgs =
        "--use-angle=d3d11 --enable-features=NvidiaVpSuperResolution,NvidiaVpTrueHDR," +
        "DirectCompositionVideoOverlays,DirectCompositionLetterboxVideoOptimization," +
        "DirectCompositionScalableVideoProcessing,DirectCompositionUseNV12DecodeSwapChain," +
        "UseD3D11VideoProcessor,D3D11VideoDecoder,HardwareAcceleratedVideoDecode,AllowDcompOverlaysInBackbuffer " +
        "--disable-features=msEdgeVideoSuperResolution,msWebOOUI,msPdfOOUI,msSmartScreenProtection " +
        "--enable-nv12-dxgi-video --ignore-gpu-blocklist --enable-gpu-rasterization --force-high-performance-gpu";

    private readonly IntPtr _hwnd;
    private CoreWebView2Controller? _controller;
    private Microsoft.UI.Dispatching.DispatcherQueueTimer? _hostFixTimer;
    private CoreWebView2? Core => _controller?.CoreWebView2;

    public MainWindow()
    {
        InitializeComponent();
        _hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);

        ExtendsContentIntoTitleBar = true;
        SetTitleBar(DragRegion);
        AppWindow.TitleBar.PreferredHeightOption = TitleBarHeightOption.Tall;
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "icon.ico"));
        AppWindow.Resize(new Windows.Graphics.SizeInt32(1280, 800));

        ContentHost.SizeChanged += (_, _) => UpdateBounds();
        ContentHost.Loaded += (_, _) => ContentHost.XamlRoot.Changed += (_, _) => UpdateBounds();
        AppWindow.Changed += OnAppWindowChanged;
        Activated += (_, e) =>
        {
            if (e.WindowActivationState != WindowActivationState.Deactivated)
            {
                _controller?.MoveFocus(CoreWebView2MoveFocusReason.Programmatic);
            }
        };
        Closed += (_, _) =>
        {
            _hostFixTimer?.Stop();
            _controller?.Close();
        };

        _ = InitWebViewAsync();
    }

    private async Task InitWebViewAsync()
    {
        // Unpackaged apps default to "<exe>.WebView2" next to the exe, which is not writable
        // under Program Files, so keep the profile in LocalAppData.
        var userData = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "com.yasin.anivox.winui", "WebView2");
        var options = new CoreWebView2EnvironmentOptions { AdditionalBrowserArguments = BrowserArgs };
        var env = await CoreWebView2Environment.CreateWithOptionsAsync(null, userData, options);

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
        core.NavigationStarting += (_, _) => Loading.IsActive = true;
        core.NavigationCompleted += (_, _) =>
        {
            Loading.IsActive = false;
            UpdateNavButtons();
            WebViewHostFix.Apply(_hwnd);
        };
        core.DocumentTitleChanged += (_, _) =>
            Title = string.IsNullOrWhiteSpace(core.DocumentTitle) ? "Anivox" : core.DocumentTitle;
        core.ContainsFullScreenElementChanged += (_, _) => SetFullScreen(core.ContainsFullScreenElement);
        core.NewWindowRequested += OnNewWindowRequested;

        await core.AddScriptToExecuteOnDocumentCreatedAsync(ReadResource("inject-windows.js"));

        core.Navigate(Home.AbsoluteUri);
        _controller.MoveFocus(CoreWebView2MoveFocusReason.Programmatic);
    }

    private static string ReadResource(string name)
    {
        using var stream = typeof(MainWindow).Assembly.GetManifestResourceStream(name)
            ?? throw new InvalidOperationException($"missing resource {name}");
        using var reader = new StreamReader(stream);
        return reader.ReadToEnd();
    }

    private void UpdateBounds()
    {
        if (_controller is null || ContentHost.XamlRoot is null)
        {
            return;
        }

        // Controller bounds are in physical pixels relative to the window client area.
        var scale = ContentHost.XamlRoot.RasterizationScale;
        var offset = ContentHost.TransformToVisual(null).TransformPoint(new Windows.Foundation.Point(0, 0));
        _controller.Bounds = new Windows.Foundation.Rect(
            Math.Round(offset.X * scale),
            Math.Round(offset.Y * scale),
            Math.Round(ContentHost.ActualWidth * scale),
            Math.Round(ContentHost.ActualHeight * scale));
    }

    private void OnAppWindowChanged(AppWindow sender, AppWindowChangedEventArgs args)
    {
        if (_controller is null)
        {
            return;
        }

        if (args.DidPositionChange)
        {
            _controller.NotifyParentWindowPositionChanged();
        }

        if (args.DidPresenterChange || args.DidSizeChange)
        {
            // Stop rendering while minimized, like a regular browser tab.
            var minimized = sender.Presenter is OverlappedPresenter p && p.State == OverlappedPresenterState.Minimized;
            _controller.IsVisible = !minimized;
        }
    }

    private static bool IsSiteHost(Uri uri) =>
        uri.Host is "anivox.fun" or "anivox.me" ||
        uri.Host.EndsWith(".anivox.fun", StringComparison.OrdinalIgnoreCase) ||
        uri.Host.EndsWith(".anivox.me", StringComparison.OrdinalIgnoreCase);

    private void OnNewWindowRequested(object? sender, CoreWebView2NewWindowRequestedEventArgs e)
    {
        if (!Uri.TryCreate(e.Uri, UriKind.Absolute, out var uri))
        {
            return;
        }

        // Site links stay in this window; sized popups (social sign-in) open as WebView2 windows;
        // other user-initiated links go to the default browser.
        if (IsSiteHost(uri))
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
        TitleBar.Visibility = fullScreen ? Visibility.Collapsed : Visibility.Visible;
        AppWindow.SetPresenter(fullScreen ? AppWindowPresenterKind.FullScreen : AppWindowPresenterKind.Default);
    }

    private void UpdateNavButtons()
    {
        BackButton.IsEnabled = Core?.CanGoBack == true;
        ForwardButton.IsEnabled = Core?.CanGoForward == true;
    }

    private void OnBack(object sender, RoutedEventArgs e) => Core?.GoBack();

    private void OnForward(object sender, RoutedEventArgs e) => Core?.GoForward();

    private void OnReload(object sender, RoutedEventArgs e) => Core?.Reload();

    private void OnHome(object sender, RoutedEventArgs e) => Core?.Navigate(Home.AbsoluteUri);
}
