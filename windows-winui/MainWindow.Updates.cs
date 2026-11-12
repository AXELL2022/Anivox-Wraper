using System;
using System.Diagnostics;
using System.Text.Json.Nodes;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Anivox.WinUI;

/// <summary>
/// Update check and changelog review (port of linux-gtk <c>updater.rs</c>). The WinUI build is not
/// a release artifact and is never replaced in place: after review the button opens the release
/// page. Right before that the version is checked again; a changed version shows its own notes
/// and needs a fresh confirmation.
/// </summary>
public sealed partial class MainWindow
{
    /// <summary>The version whose notes are on screen: only it can be confirmed.</summary>
    private string? _reviewedVersion;

    private string _releasePage = "https://github.com/AXELL2022/Anivox-Wraper/releases/latest";

    private void InitUpdates()
    {
        UpdateCurrent.Text = $"Установлена версия {VersionText.Text}";
        // Background check shortly after launch, like the Tauri and GTK builds.
        var timer = DispatcherQueue.CreateTimer();
        timer.IsRepeating = false;
        timer.Interval = TimeSpan.FromSeconds(3);
        timer.Tick += async (_, _) =>
        {
            var result = await CheckUpdateAsync();
            if (result.Ok)
            {
                ShowUpdate(result.Update);
            }
            else
            {
                Debug.WriteLine(result.Error);
            }
        };
        timer.Start();
    }

    private readonly record struct UpdateCheck(bool Ok, JsonObject? Update, string? Error);

    private async Task<UpdateCheck> CheckUpdateAsync()
    {
        UpdateOpen.IsEnabled = false;
        UpdateRetry.IsEnabled = false;
        var result = await Task.Run(_core.CheckUpdate);
        UpdateRetry.IsEnabled = true;
        UpdateOpen.IsEnabled = _reviewedVersion is not null;
        if (!result.Ok)
        {
            return new UpdateCheck(false, null, result.Text);
        }
        var value = JsonNode.Parse(result.Text) as JsonObject;
        if (value?["releasePage"]?.GetValue<string>() is { } page)
        {
            _releasePage = page;
        }
        return new UpdateCheck(true, value?["update"] as JsonObject, null);
    }

    private void ShowUpdate(JsonObject? update)
    {
        var version = update?["version"]?.GetValue<string>();
        if (version is not null)
        {
            UpdateButton.Content = $"v{version}";
            ToolTipService.SetToolTip(UpdateButton, $"Доступно обновление v{version}");
            UpdateTitle.Text = $"Anivox v{version}";
            var notes = update!["notes"]?.GetValue<string>()?.Trim();
            UpdateNotes.Text = string.IsNullOrEmpty(notes) ? "Описание изменений не указано." : notes;
            UpdateButton.Visibility = Visibility.Visible;
        }
        else
        {
            UpdateTitle.Text = "Обновление Anivox";
        }
        _reviewedVersion = version;
        UpdateOpen.IsEnabled = version is not null;
    }

    /// <summary>Re-checks right before acting so an unreviewed version is never opened.</summary>
    private async void OnUpdateOpen(object sender, RoutedEventArgs e)
    {
        var expected = _reviewedVersion;
        if (expected is null)
        {
            return;
        }
        UpdateStatus.Text = "Проверка версии…";
        var result = await CheckUpdateAsync();
        var version = result.Update?["version"]?.GetValue<string>();
        if (!result.Ok)
        {
            UpdateStatus.Text = result.Error;
        }
        else if (version == expected)
        {
            try
            {
                Process.Start(new ProcessStartInfo(_releasePage) { UseShellExecute = true });
                UpdateStatus.Text = "Открыта страница выпуска. Скачайте и установите новую версию, затем перезапустите Anivox.";
            }
            catch (Exception ex)
            {
                UpdateStatus.Text = $"Не удалось открыть браузер: {ex.Message}";
            }
        }
        else if (version is not null)
        {
            ShowUpdate(result.Update);
            UpdateStatus.Text = "Вышла другая версия. Ознакомьтесь с изменениями и подтвердите ещё раз.";
        }
        else
        {
            ShowUpdate(null);
            UpdateStatus.Text = "Обновление больше не доступно.";
        }
    }

    private async void OnUpdateRetry(object sender, RoutedEventArgs e)
    {
        UpdateStatus.Text = "Проверка обновлений…";
        var result = await CheckUpdateAsync();
        if (!result.Ok)
        {
            UpdateStatus.Text = result.Error;
            return;
        }
        ShowUpdate(result.Update);
        UpdateStatus.Text = result.Update is not null
            ? "Перед установкой ознакомьтесь с изменениями."
            : "У вас последняя версия.";
    }

    private void OnUpdateLater(object sender, RoutedEventArgs e) => UpdateFlyout.Hide();
}
