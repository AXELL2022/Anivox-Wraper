using System;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Anivox.WinUI;

/// <summary>
/// "MPV" toolbar button (port of linux-gtk <c>player_button.rs</c>): hands the current episode to
/// MPV through the site script and shows the player status it reports (<c>report_player_status</c>).
/// Longer explanations go to the message next to it.
/// </summary>
public sealed partial class MainWindow
{
    private const string MpvText = "MPV";
    private static readonly TimeSpan PlayerFallback = TimeSpan.FromSeconds(10);
    private static readonly TimeSpan PlayerMessageTime = TimeSpan.FromSeconds(5);

    private DispatcherQueueTimer _mpvResetTimer = null!;
    private DispatcherQueueTimer _messageTimer = null!;
    private bool _mpvTimedOutLaunch;

    private void InitPlayer()
    {
        _mpvResetTimer = DispatcherQueue.CreateTimer();
        _mpvResetTimer.IsRepeating = false;
        _mpvResetTimer.Tick += (_, _) =>
        {
            ResetMpvButton();
            if (_mpvTimedOutLaunch)
            {
                ShowPlayerMessage("MPV: сначала загрузите сайт и включите серию.");
            }
        };
        _messageTimer = DispatcherQueue.CreateTimer();
        _messageTimer.IsRepeating = false;
        _messageTimer.Tick += (_, _) => PlayerMessage.Visibility = Visibility.Collapsed;
    }

    private async void OnMpv(object sender, RoutedEventArgs e)
    {
        var core = Core;
        if (core is null || !AnivoxCore.IsSiteUrl(core.Source))
        {
            ShowPlayerMessage("MPV: сначала загрузите сайт и включите серию.");
            return;
        }

        MpvButton.IsEnabled = false;
        SetMpvLabel("MPV · …", null);
        // Without any status from the page in time, the launch did not start.
        RestartMpvTimer(PlayerFallback, timedOutLaunch: true);
        try
        {
            await core.ExecuteScriptAsync("window.__anivoxLaunchMpv?.()");
        }
        catch (Exception)
        {
            // The page went away; the fallback timer resets the button.
        }
    }

    /// <summary><c>resetAfterMs == 0</c> keeps the button busy until the next status.</summary>
    private void ShowPlayerStatus(string text, string? color, ulong resetAfterMs, string? detail)
    {
        if (!string.IsNullOrEmpty(detail))
        {
            ShowPlayerMessage(detail);
        }
        SetMpvLabel(ButtonText(text), color);
        MpvButton.IsEnabled = resetAfterMs > 0;
        RestartMpvTimer(resetAfterMs > 0 ? TimeSpan.FromMilliseconds(resetAfterMs) : PlayerFallback,
            timedOutLaunch: false);
    }

    private void ShowPlayerMessage(string text)
    {
        PlayerMessage.Text = text;
        ToolTipService.SetToolTip(PlayerMessage, text);
        PlayerMessage.Visibility = Visibility.Visible;
        _messageTimer.Stop();
        _messageTimer.Interval = PlayerMessageTime;
        _messageTimer.Start();
    }

    private void RestartMpvTimer(TimeSpan delay, bool timedOutLaunch)
    {
        _mpvTimedOutLaunch = timedOutLaunch;
        _mpvResetTimer.Stop();
        _mpvResetTimer.Interval = delay;
        _mpvResetTimer.Start();
    }

    private void ResetMpvButton()
    {
        SetMpvLabel(MpvText, null);
        MpvButton.IsEnabled = true;
    }

    private void SetMpvLabel(string text, string? color)
    {
        MpvLabel.Text = text;
        if (ParseColor(color) is { } brush)
        {
            MpvLabel.Foreground = brush;
        }
        else
        {
            MpvLabel.ClearValue(TextBlock.ForegroundProperty);
        }
    }

    /// <summary>The button has its own icon: drop the status symbol the site puts first.</summary>
    internal static string ButtonText(string text)
    {
        var trimmed = text.TrimStart('▶', '✔', '⚠', '✨');
        return trimmed.Length == text.Length ? text : trimmed.TrimStart();
    }

    /// <summary>Colors come from the page: accept only <c>#rgb</c>, <c>#rrggbb</c> and <c>#rrggbbaa</c>.</summary>
    internal static SolidColorBrush? ParseColor(string? color)
    {
        if (color is null || !color.StartsWith('#'))
        {
            return null;
        }
        var hex = color[1..];
        if (hex.Length == 3)
        {
            hex = $"{hex[0]}{hex[0]}{hex[1]}{hex[1]}{hex[2]}{hex[2]}";
        }
        if (hex.Length == 6)
        {
            hex += "FF";
        }
        if (hex.Length != 8 || !uint.TryParse(hex, System.Globalization.NumberStyles.HexNumber, null, out var rgba))
        {
            return null;
        }
        return new SolidColorBrush(Microsoft.UI.ColorHelper.FromArgb(
            (byte)rgba, (byte)(rgba >> 24), (byte)(rgba >> 16), (byte)(rgba >> 8)));
    }
}
