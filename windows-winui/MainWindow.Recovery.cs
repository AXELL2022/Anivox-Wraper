using System;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.Web.WebView2.Core;

namespace Anivox.WinUI;

/// <summary>
/// Loading watchdog and the recovery screen (port of linux-gtk <c>site_recovery.rs</c>).
/// A navigation starts a new attempt (generation, kept by the core so <c>site_document_state</c>
/// reports can be matched). <c>site-lifecycle.js</c> reports when the site's app has mounted; after
/// 25 s without that the screen offers to retry, open the mirror or the home page, or keep waiting.
/// A timeout is not proof of a network failure, so a slow page may still finish later.
/// Unlike GTK the screen is not shown during a normal load: it would have to hide the site HWND.
/// </summary>
public sealed partial class MainWindow
{
    private const string MirrorUrl = "https://anivox.me/";
    private static readonly TimeSpan Watchdog = TimeSpan.FromSeconds(25);

    private enum LoadPhase
    {
        Loading,
        Ready,
        Timeout,
        Error,
    }

    private LoadPhase _loadPhase = LoadPhase.Ready;
    private ulong _loadGeneration;
    private ulong? _recoveryDismissed;
    private string? _loadDetail;
    private DispatcherQueueTimer? _watchdogTimer;

    private bool RecoveryVisible => _loadPhase is LoadPhase.Timeout or LoadPhase.Error
        && _recoveryDismissed != _loadGeneration;

    /// <summary>A new top-level navigation of the site view: arm the watchdog.</summary>
    private void BeginLoadAttempt()
    {
        _loadGeneration = _core.BeginLoad();
        _loadPhase = LoadPhase.Loading;
        _loadDetail = null;
        var generation = _loadGeneration;
        _watchdogTimer?.Stop();
        _watchdogTimer = DispatcherQueue.CreateTimer();
        _watchdogTimer.IsRepeating = false;
        _watchdogTimer.Interval = Watchdog;
        _watchdogTimer.Tick += (_, _) =>
        {
            if (_loadGeneration == generation && _loadPhase == LoadPhase.Loading)
            {
                _loadPhase = LoadPhase.Timeout;
                RenderRecovery();
            }
        };
        _watchdogTimer.Start();
        RenderRecovery();
    }

    /// <summary>Only the current attempt can finish; a late report of an older one is ignored.</summary>
    private void FinishLoadAttempt(ulong generation)
    {
        if (generation != _loadGeneration)
        {
            return;
        }
        _watchdogTimer?.Stop();
        _loadPhase = LoadPhase.Ready;
        _loadDetail = null;
        RenderRecovery();
    }

    private void OnSiteNavigationCompleted(CoreWebView2 view, CoreWebView2NavigationCompletedEventArgs args)
    {
        if (!args.IsSuccess)
        {
            // A newer navigation or a download cancels the old load: not a failure.
            if (args.WebErrorStatus is CoreWebView2WebErrorStatus.OperationCanceled)
            {
                return;
            }
            _watchdogTimer?.Stop();
            _loadPhase = LoadPhase.Error;
            _loadDetail = $"{view.Source}\n{args.WebErrorStatus}" +
                (args.HttpStatusCode > 0 ? $" (HTTP {args.HttpStatusCode})" : "");
            RenderRecovery();
            return;
        }

        // A finished real document on a foreign host (e.g. a login page) also ends the wait:
        // site-lifecycle.js runs only on the site itself.
        if (Uri.TryCreate(view.Source, UriKind.Absolute, out var uri)
            && uri.Scheme is "https" or "http"
            && !AnivoxCore.IsSiteUrl(view.Source))
        {
            FinishLoadAttempt(_loadGeneration);
        }
    }

    private void RenderRecovery()
    {
        var visible = RecoveryVisible;
        RecoveryLayer.Visibility = visible ? Visibility.Visible : Visibility.Collapsed;
        RecoverySpinner.IsActive = visible && _loadPhase == LoadPhase.Loading;
        (RecoveryTitle.Text, RecoveryDescription.Text) = _loadPhase switch
        {
            LoadPhase.Timeout => ("Сайт не ответил вовремя",
                "Сайт или сеть могут работать медленно. Можно повторить загрузку, открыть зеркало или главную, либо продолжить ожидание."),
            LoadPhase.Error => ("Не удалось загрузить Anivox",
                "При загрузке страницы произошла ошибка. Попробуйте ещё раз, откройте зеркало или перейдите на главную."),
            LoadPhase.Ready => ("Anivox готов", "Возвращаемся к просмотру…"),
            _ => ("Загрузка Anivox…", "Подключаемся к сайту. Это может занять немного времени."),
        };
        RecoveryDetail.Text = _loadDetail ?? "";
        RecoveryDetailBox.Visibility = _loadDetail is null ? Visibility.Collapsed : Visibility.Visible;
        UpdateWebViewVisibility();
    }

    private void OnRecoveryRetry(object sender, RoutedEventArgs e)
    {
        var current = Core?.Source;
        var reloadable = current is not null
            && (current.StartsWith("https://", StringComparison.Ordinal) || current.StartsWith("http://", StringComparison.Ordinal));
        Core?.Navigate(reloadable ? current! : HomeUrl);
    }

    private void OnRecoveryMirror(object sender, RoutedEventArgs e) => Core?.Navigate(MirrorUrl);

    private void OnRecoveryHome(object sender, RoutedEventArgs e) => Core?.Navigate(HomeUrl);

    private void OnRecoveryWait(object sender, RoutedEventArgs e)
    {
        _recoveryDismissed = _loadGeneration;
        RenderRecovery();
    }
}
