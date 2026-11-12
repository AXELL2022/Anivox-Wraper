using System;
using System.Text.Json.Nodes;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Anivox.WinUI;

/// <summary>
/// Connection check flyout (port of linux-gtk <c>connection.rs</c>): compares the passive video
/// segment statistics from <c>segment-monitor.js</c> with a speed test; the verdicts come from the core.
/// </summary>
public sealed partial class MainWindow
{
    private JsonNode? _connNet;
    private bool _connMeasuring;
    private DispatcherQueueTimer? _connTimer;

    /// <summary>Bumped on every open/close so a late measurement of a closed flyout is dropped.</summary>
    private int _connEpoch;

    private void OnConnectionOpening(object? sender, object e)
    {
        _connEpoch++;
        _connNet = null;
        RenderConnection();
        _connTimer?.Stop();
        _connTimer = DispatcherQueue.CreateTimer();
        _connTimer.Interval = TimeSpan.FromSeconds(1);
        _connTimer.Tick += (_, _) => RenderConnection();
        _connTimer.Start();
        _ = MeasureConnectionAsync();
    }

    private void OnConnectionClosed(object? sender, object e)
    {
        _connEpoch++;
        _connTimer?.Stop();
        _connTimer = null;
    }

    private void OnConnectionRecheck(object sender, RoutedEventArgs e)
    {
        _connNet = null;
        _ = MeasureConnectionAsync();
    }

    private async Task MeasureConnectionAsync()
    {
        if (_connMeasuring)
        {
            return;
        }
        _connMeasuring = true;
        ConnRecheck.IsEnabled = false;
        RenderConnection();
        var epoch = _connEpoch;
        var result = await Task.Run(_core.MeasureInternet)
            ?? new JsonObject { ["ok"] = false, ["mbps"] = 0.0 };
        _connMeasuring = false;
        ConnRecheck.IsEnabled = true;
        if (epoch == _connEpoch)
        {
            _connNet = result;
        }
        RenderConnection();
    }

    private void RenderConnection()
    {
        var view = _core.ConnectionView(_connNet, _connMeasuring);
        if (view is null)
        {
            return;
        }
        if (view["internet"] is JsonObject internet)
        {
            Paint(ConnInternet, internet);
        }
        if (view["video"] is JsonObject video)
        {
            Paint(ConnVideo, video);
        }
        var detail = view["detail"]?.GetValue<string>();
        ConnDetail.Text = detail ?? "";
        ConnDetail.Visibility = detail is null ? Visibility.Collapsed : Visibility.Visible;
        if (view["verdict"] is JsonObject verdict)
        {
            ConnVerdictTitle.Text = verdict["title"]?.GetValue<string>() ?? "";
            ConnVerdictTitle.Foreground = KindBrush(verdict["kind"]?.GetValue<string>());
            ConnVerdictText.Text = verdict["text"]?.GetValue<string>() ?? "";
        }
    }

    private static void Paint(TextBlock block, JsonObject line)
    {
        block.Text = line["text"]?.GetValue<string>() ?? "—";
        block.Foreground = KindBrush(line["kind"]?.GetValue<string>());
    }

    private static Brush KindBrush(string? kind) => (Brush)Application.Current.Resources[kind switch
    {
        "good" => "SystemFillColorSuccessBrush",
        "bad" => "SystemFillColorCriticalBrush",
        "warn" => "SystemFillColorCautionBrush",
        _ => "TextFillColorSecondaryBrush",
    }];
}
