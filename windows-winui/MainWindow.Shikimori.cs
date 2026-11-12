using System;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.Web.WebView2.Core;
using Windows.Storage.Streams;

namespace Anivox.WinUI;

/// <summary>
/// Shikimori overlay (port of linux-gtk <c>shikimori.rs</c>). Opened lazily: the same title as the
/// Anivox page or the Shikimori home page. The button, Escape and a click on the backdrop close
/// it, and closing destroys the view, so the next opening starts fresh. Shikimori gets no Anivox
/// IPC: its only message closes the overlay. Engine-managed popups stay for social sign-in.
/// </summary>
public sealed partial class MainWindow
{
    /// <summary>Escape in the page closes the overlay unless the page handled the key itself.</summary>
    private const string ShikimoriEscapeScript = """
        (() => {
            if (location.origin !== 'https://shikimori.io' || window !== window.top) return;
            const webview = window.chrome && window.chrome.webview;
            if (!webview) return;
            document.addEventListener('keydown', event => {
                if (event.key === 'Escape' && !event.defaultPrevented) {
                    event.preventDefault();
                    webview.postMessage('close');
                }
            });
        })();
        """;

    private CoreWebView2Controller? _shikimori;
    private DispatcherQueueTimer? _shikimoriTimer;
    private bool _shikimoriOpening;

    private void OnShikimoriToggle(object sender, RoutedEventArgs e)
    {
        if (ShikimoriButton.IsChecked == true)
        {
            _ = OpenShikimoriAsync();
        }
        else
        {
            CloseShikimori();
        }
    }

    private async Task OpenShikimoriAsync()
    {
        if (_shikimori is not null || _shikimoriOpening || _env is null || _controller is null)
        {
            ShikimoriButton.IsChecked = _shikimori is not null;
            return;
        }

        _shikimoriOpening = true;
        try
        {
            await ShowSiteSnapshotAsync();
            ShikimoriTitle.Text = "Shikimori";
            ShikimoriBack.IsEnabled = false;
            ShikimoriForward.IsEnabled = false;
            ShikimoriLayer.Visibility = Visibility.Visible;
            ShikimoriLayer.UpdateLayout();

            var controller = await _env.CreateCoreWebView2ControllerAsync(
                CoreWebView2ControllerWindowReference.CreateFromWindowHandle((ulong)_hwnd));
            if (ShikimoriButton.IsChecked != true)
            {
                // Closed while the view was being created.
                controller.Close();
                ShikimoriLayer.Visibility = Visibility.Collapsed;
                return;
            }

            _shikimori = controller;
            controller.DefaultBackgroundColor = Microsoft.UI.ColorHelper.FromArgb(255, 32, 32, 32);
            PlaceOver(controller, ShikimoriHost);
            UpdateWebViewVisibility();
            WebViewHostFix.Apply(_hwnd);

            var view = controller.CoreWebView2;
            view.Settings.IsStatusBarEnabled = false;
            // Social sign-in redirects away from Shikimori and back, always over HTTPS.
            view.NavigationStarting += (_, args) =>
            {
                if (!args.Uri.StartsWith("https://", StringComparison.Ordinal) && args.Uri != "about:blank")
                {
                    args.Cancel = true;
                }
            };
            view.HistoryChanged += (_, _) =>
            {
                ShikimoriBack.IsEnabled = view.CanGoBack;
                ShikimoriForward.IsEnabled = view.CanGoForward;
            };
            view.DocumentTitleChanged += (_, _) =>
                ShikimoriTitle.Text = string.IsNullOrWhiteSpace(view.DocumentTitle) ? "Shikimori" : view.DocumentTitle;
            view.NavigationCompleted += (_, _) => WebViewHostFix.Apply(_hwnd);
            view.WebMessageReceived += (_, args) =>
            {
                // Only Shikimori itself may close the overlay, never a sign-in provider.
                if (AnivoxCore.IsShikimoriUrl(args.Source))
                {
                    CloseShikimori();
                }
            };
            await view.AddScriptToExecuteOnDocumentCreatedAsync(ShikimoriEscapeScript);
            view.Navigate(AnivoxCore.ShikimoriStartUrl(Core?.Source));
            controller.MoveFocus(CoreWebView2MoveFocusReason.Programmatic);

            _shikimoriTimer = DispatcherQueue.CreateTimer();
            _shikimoriTimer.Interval = TimeSpan.FromSeconds(2);
            _shikimoriTimer.Tick += (_, _) => ReportShikimoriActivity();
            _shikimoriTimer.Start();
            ReportShikimoriActivity();
            ToolTipService.SetToolTip(ShikimoriButton, "Закрыть Shikimori");
        }
        catch (Exception ex)
        {
            ShowPlayerMessage($"Shikimori: {ex.Message}");
            CloseShikimori();
        }
        finally
        {
            _shikimoriOpening = false;
        }
    }

    /// <summary>A dimmed picture of the site stands in for it while its HWND is hidden.</summary>
    private async Task ShowSiteSnapshotAsync()
    {
        ShikimoriSnapshot.Source = null;
        var core = Core;
        if (core is null)
        {
            return;
        }
        try
        {
            using var stream = new InMemoryRandomAccessStream();
            await core.CapturePreviewAsync(CoreWebView2CapturePreviewImageFormat.Jpeg, stream);
            stream.Seek(0);
            var image = new BitmapImage();
            await image.SetSourceAsync(stream);
            ShikimoriSnapshot.Source = image;
        }
        catch (Exception)
        {
            // No picture: the backdrop stays plain.
        }
    }

    private void CloseShikimori()
    {
        _shikimoriTimer?.Stop();
        _shikimoriTimer = null;
        var controller = _shikimori;
        _shikimori = null;
        controller?.Close();
        ShikimoriLayer.Visibility = Visibility.Collapsed;
        ShikimoriSnapshot.Source = null;
        ShikimoriButton.IsChecked = false;
        ToolTipService.SetToolTip(ShikimoriButton, "Открыть Shikimori поверх сайта");
        UpdateWebViewVisibility();
        _controller?.MoveFocus(CoreWebView2MoveFocusReason.Programmatic);
        if (controller is not null)
        {
            SendShikimoriActivity(null);
        }
    }

    private void ReportShikimoriActivity()
    {
        var source = _shikimori?.CoreWebView2?.Source;
        if (source is not null)
        {
            SendShikimoriActivity(source);
        }
    }

    /// <summary>Only the kind of page reaches Discord; null restores the embedded player status.</summary>
    private void SendShikimoriActivity(string? pageUri)
    {
        Task.Run(() =>
        {
            var result = _core.SetShikimoriActivity(pageUri);
            if (!result.Ok)
            {
                System.Diagnostics.Debug.WriteLine($"Discord: {result.Text}");
            }
        });
    }

    private void OnShikimoriBackdropTapped(object sender, TappedRoutedEventArgs e) => CloseShikimori();

    private void OnShikimoriClose(object sender, RoutedEventArgs e) => CloseShikimori();

    private void OnShikimoriEscape(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        args.Handled = true;
        CloseShikimori();
    }

    private void OnShikimoriBack(object sender, RoutedEventArgs e) => _shikimori?.CoreWebView2?.GoBack();

    private void OnShikimoriForward(object sender, RoutedEventArgs e) => _shikimori?.CoreWebView2?.GoForward();

    private void OnShikimoriReload(object sender, RoutedEventArgs e) => _shikimori?.CoreWebView2?.Reload();

    private void OnShikimoriHostSizeChanged(object sender, SizeChangedEventArgs e)
    {
        if (_shikimori is not null)
        {
            PlaceOver(_shikimori, ShikimoriHost);
        }
    }
}
