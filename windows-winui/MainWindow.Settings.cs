using System;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Text.Json.Nodes;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.Windows.Storage.Pickers;

namespace Anivox.WinUI;

/// <summary>
/// Native settings flyout (port of linux-gtk <c>settings.rs</c>): startup page, MPV path, embedded
/// mirroring and clearing saved positions. Remote pages have no IPC command for any of this.
/// </summary>
public sealed partial class MainWindow
{
    /// <summary>An unsaved path picked in the file dialog survives reopening the flyout.</summary>
    private bool _settingsDirty;

    /// <summary>Ignore control events while values are filled in programmatically.</summary>
    private bool _settingsLoading;

    private void InitSettings()
    {
        var version = Assembly.GetExecutingAssembly().GetName().Version;
        VersionText.Text = version is null ? "" : $"v{version.Major}.{version.Minor}.{version.Build}";
        LogsPath.Text = AnivoxCore.LogsDir;

        // Keep the title bar's own buttons clear of the system caption buttons.
        TitleBar.Loaded += (_, _) =>
        {
            UpdateCaptionInset();
            TitleBar.XamlRoot.Changed += (_, _) => UpdateCaptionInset();
        };
        AppWindow.Changed += (_, args) =>
        {
            if (args.DidSizeChange || args.DidPresenterChange)
            {
                UpdateCaptionInset();
            }
        };
    }

    private void UpdateCaptionInset()
    {
        var scale = TitleBar.XamlRoot?.RasterizationScale ?? 1.0;
        CaptionInset.Width = new GridLength(AppWindow.TitleBar.RightInset / scale);
    }

    private void OnSettingsOpening(object? sender, object e)
    {
        _settingsLoading = true;
        var startup = _session["startupPage"]?.GetValue<string>() == "home" ? 1 : 0;
        StartupPage.SelectedIndex = startup;
        SessionStatus.Text = "";
        _settingsLoading = false;
        if (_settingsDirty)
        {
            return;
        }

        var result = _core.LoadPlayerSettings();
        if (result.Ok && JsonNode.Parse(result.Text) is JsonObject settings)
        {
            _settingsLoading = true;
            MpvPath.Text = settings["mpvPath"]?.GetValue<string>() ?? "";
            MirrorEmbedded.IsChecked = settings["mirrorEmbedded"]?.GetValue<bool>() == true;
            _settingsLoading = false;
            _settingsDirty = false;
            SettingsStatus.Text = "";
        }
        else
        {
            SettingsStatus.Text = result.Text;
        }
    }

    private void OnStartupPageChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_settingsLoading || StartupPage.SelectedItem is not ComboBoxItem { Tag: string page })
        {
            return;
        }

        _session["startupPage"] = page;
        var result = FlushSession();
        SessionStatus.Text = result.Ok
            ? "Вариант запуска сохранён."
            : $"Не удалось сохранить запуск: {result.Text}";
    }

    private void OnSettingsEdited(object sender, RoutedEventArgs e)
    {
        if (!_settingsLoading)
        {
            _settingsDirty = true;
        }
    }

    private async void OnBrowseMpv(object sender, RoutedEventArgs e)
    {
        var picker = new FileOpenPicker(AppWindow.Id)
        {
            SuggestedStartLocation = PickerLocationId.ComputerFolder,
            CommitButtonText = "Выбрать",
        };
        picker.FileTypeFilter.Add(".exe");
        SettingsFlyout.Hide();
        try
        {
            var picked = await picker.PickSingleFileAsync();
            if (picked is not null)
            {
                MpvPath.Text = picked.Path;
                _settingsDirty = true;
                SettingsStatus.Text = "Нажмите «Сохранить», чтобы применить путь.";
            }
        }
        catch (Exception ex)
        {
            SettingsStatus.Text = $"Не удалось открыть выбор файла: {ex.Message}";
        }
        SettingsFlyout.ShowAt(SettingsButton);
    }

    private void OnResetMpvPath(object sender, RoutedEventArgs e)
    {
        MpvPath.Text = "";
        _settingsDirty = true;
        SettingsStatus.Text = "Нажмите «Сохранить», чтобы использовать MPV из PATH.";
    }

    private void OnSaveSettings(object sender, RoutedEventArgs e)
    {
        var path = MpvPath.Text.Trim();
        var result = _core.SavePlayerSettings(path.Length == 0 ? null : path, MirrorEmbedded.IsChecked == true);
        if (result.Ok)
        {
            _settingsDirty = false;
            SettingsStatus.Text = "Настройки сохранены.";
        }
        else
        {
            SettingsStatus.Text = result.Text;
        }
    }

    private async void OnClearHistory(object sender, RoutedEventArgs e)
    {
        ClearHistoryButton.IsEnabled = false;
        var result = await Task.Run(_core.ClearPlaybackHistory);
        SettingsStatus.Text = result.Ok ? "Сохранённые позиции удалены." : result.Text;
        ClearHistoryButton.IsEnabled = true;
    }

    private void OnOpenLogs(object sender, RoutedEventArgs e)
    {
        try
        {
            Directory.CreateDirectory(AnivoxCore.LogsDir);
            Process.Start(new ProcessStartInfo(AnivoxCore.LogsDir) { UseShellExecute = true });
        }
        catch (Exception ex)
        {
            SettingsStatus.Text = $"Не удалось открыть папку журналов: {ex.Message}";
        }
    }
}
