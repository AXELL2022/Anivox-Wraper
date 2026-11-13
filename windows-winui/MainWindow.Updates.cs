using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Text.Json.Nodes;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Anivox.WinUI;

/// <summary>
/// Update check, changelog review and installation. When the release carries an installer for
/// this build, the button downloads it: the core re-checks <c>latest.json</c> first (a changed
/// version shows its own notes and needs a fresh confirmation), downloads only from this
/// repository's releases and accepts the file only with a valid release-key signature. The
/// verified installer is started on a second click, after which Anivox exits. Without an
/// installer in the release the button opens the release page instead.
/// </summary>
public sealed partial class MainWindow
{
    /// <summary>
    /// Arguments for the NSIS installer: passive mode, update run. The old Tauri updater starts
    /// the same installer with these flags, so it has to accept them anyway.
    /// </summary>
    private const string InstallerArgs = "/P /UPDATE";

    /// <summary>The release whose notes are on screen: only it can be downloaded or opened.</summary>
    private JsonObject? _reviewed;
    private string? _reviewedVersion;
    /// <summary>The release has an installer this build may download and run.</summary>
    private bool _installable;
    private bool _downloading;
    /// <summary>A verified installer for <see cref="_downloadedVersion"/>.</summary>
    private string? _downloadedPath;
    private string? _downloadedVersion;

    private string _releasePage = "https://github.com/AXELL2022/Anivox-Wraper/releases/latest";

    private void InitUpdates()
    {
        UpdateCurrent.Text = $"Установлена версия {VersionText.Text}";
        // Background check shortly after launch, like the GTK build.
        var timer = DispatcherQueue.CreateTimer();
        timer.IsRepeating = false;
        timer.Interval = TimeSpan.FromSeconds(3);
        timer.Tick += async (_, _) =>
        {
            var result = await CheckUpdateAsync();
            if (result.Ok)
            {
                ShowUpdate(result.Update, result.Installable);
            }
            else
            {
                Debug.WriteLine(result.Error);
            }
        };
        timer.Start();
    }

    private readonly record struct UpdateCheck(bool Ok, JsonObject? Update, bool Installable, string? Error);

    private async Task<UpdateCheck> CheckUpdateAsync()
    {
        SetUpdateBusy(true);
        var result = await Task.Run(_core.CheckUpdate);
        SetUpdateBusy(false);
        if (!result.Ok)
        {
            return new UpdateCheck(false, null, false, result.Text);
        }
        var value = JsonNode.Parse(result.Text) as JsonObject;
        if (value?["releasePage"]?.GetValue<string>() is { } page)
        {
            _releasePage = page;
        }
        var installable = value?["installable"]?.GetValue<bool>() == true;
        return new UpdateCheck(true, value?["update"] as JsonObject, installable, null);
    }

    private void SetUpdateBusy(bool busy)
    {
        UpdateOpen.IsEnabled = !busy && _reviewedVersion is not null;
        UpdateRetry.IsEnabled = !busy;
    }

    private void ShowUpdate(JsonObject? update, bool installable)
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
        _reviewed = version is null ? null : update;
        _reviewedVersion = version;
        _installable = installable && version is not null;
        if (version is null || version != _downloadedVersion || !File.Exists(_downloadedPath))
        {
            _downloadedPath = null;
            _downloadedVersion = null;
        }
        UpdateOpen.Content = _downloadedPath is not null
            ? "Установить и перезапустить"
            : _installable ? "Скачать и установить" : "Открыть страницу обновления";
        UpdateOpen.IsEnabled = version is not null && !_downloading;
    }

    private void OnUpdateOpen(object sender, RoutedEventArgs e)
    {
        if (_reviewed is null || _downloading)
        {
            return;
        }
        if (_downloadedPath is not null)
        {
            InstallUpdate(_downloadedPath);
        }
        else if (_installable)
        {
            _ = DownloadUpdateAsync(_reviewed);
        }
        else
        {
            _ = OpenReleasePageAfterCheckAsync();
        }
    }

    /// <summary>Downloads and verifies the reviewed installer; a changed release is shown again.</summary>
    private async Task DownloadUpdateAsync(JsonObject reviewed)
    {
        _downloading = true;
        UpdateOpen.IsEnabled = false;
        UpdateRetry.IsEnabled = false;
        UpdateCancel.Visibility = Visibility.Visible;
        UpdateCancel.IsEnabled = true;
        UpdateProgress.IsIndeterminate = true;
        UpdateProgress.Visibility = Visibility.Visible;
        UpdateStatus.Text = "Проверка версии…";
        var json = reviewed.ToJsonString();
        var version = _reviewedVersion;
        try
        {
            var result = await Task.Run(() => _core.DownloadUpdate(json,
                (received, total) => DispatcherQueue.TryEnqueue(() => ShowDownloadProgress(received, total))));
            if (!result.Ok)
            {
                UpdateStatus.Text = result.Text == "Загрузка отменена"
                    ? "Загрузка отменена."
                    : $"{result.Text}. Попробуйте ещё раз или скачайте обновление со страницы выпуска.";
                return;
            }
            var value = JsonNode.Parse(result.Text) as JsonObject;
            if (value?["path"]?.GetValue<string>() is { } path)
            {
                _downloadedPath = path;
                _downloadedVersion = version;
                UpdateStatus.Text = $"Обновление v{version} скачано, подпись проверена. " +
                    "Нажмите «Установить и перезапустить»: Anivox закроется и запустит установщик.";
            }
            else if (value?["changed"] is JsonObject changed)
            {
                _downloading = false;
                ShowUpdate(changed, value["installable"]?.GetValue<bool>() == true);
                UpdateStatus.Text = "Вышла другая версия. Ознакомьтесь с изменениями и подтвердите ещё раз.";
            }
            else
            {
                _downloading = false;
                ShowUpdate(null, false);
                UpdateStatus.Text = "Обновление больше не доступно.";
            }
        }
        finally
        {
            _downloading = false;
            UpdateCancel.Visibility = Visibility.Collapsed;
            UpdateProgress.Visibility = Visibility.Collapsed;
            UpdateRetry.IsEnabled = true;
            ShowUpdate(_reviewed, _installable);
        }
    }

    private void ShowDownloadProgress(ulong received, ulong total)
    {
        if (!_downloading)
        {
            return;
        }
        static string Mb(ulong bytes) => (bytes / 1048576.0).ToString("0.0");
        if (total > 0)
        {
            UpdateProgress.IsIndeterminate = false;
            UpdateProgress.Value = Math.Min(100.0, received * 100.0 / total);
            UpdateStatus.Text = $"Скачивание… {Mb(received)} из {Mb(total)} МБ";
        }
        else
        {
            UpdateStatus.Text = $"Скачивание… {Mb(received)} МБ";
        }
    }

    private void OnUpdateCancel(object sender, RoutedEventArgs e)
    {
        _core.CancelUpdate();
        UpdateCancel.IsEnabled = false;
        UpdateStatus.Text = "Отмена…";
    }

    /// <summary>Starts the verified installer and exits so it can replace the files.</summary>
    private void InstallUpdate(string path)
    {
        if (!File.Exists(path))
        {
            _downloadedPath = null;
            ShowUpdate(_reviewed, _installable);
            UpdateStatus.Text = "Скачанный файл не найден. Скачайте обновление ещё раз.";
            return;
        }
        try
        {
            // Shell execution lets Windows ask for elevation if the installer needs it.
            Process.Start(new ProcessStartInfo(path, InstallerArgs) { UseShellExecute = true });
        }
        catch (Win32Exception ex) when (ex.NativeErrorCode == 1223)
        {
            UpdateStatus.Text = "Установка отменена.";
            return;
        }
        catch (Exception ex)
        {
            UpdateStatus.Text = $"Не удалось запустить установщик: {ex.Message}";
            return;
        }
        ExitApp();
    }

    /// <summary>Re-checks right before acting so an unreviewed version is never opened.</summary>
    private async Task OpenReleasePageAfterCheckAsync()
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
            OpenReleasePage("Открыта страница выпуска. Скачайте и установите новую версию, затем перезапустите Anivox.");
        }
        else if (version is not null)
        {
            ShowUpdate(result.Update, result.Installable);
            UpdateStatus.Text = "Вышла другая версия. Ознакомьтесь с изменениями и подтвердите ещё раз.";
        }
        else
        {
            ShowUpdate(null, false);
            UpdateStatus.Text = "Обновление больше не доступно.";
        }
    }

    private void OpenReleasePage(string status)
    {
        try
        {
            Process.Start(new ProcessStartInfo(_releasePage) { UseShellExecute = true });
            UpdateStatus.Text = status;
        }
        catch (Exception ex)
        {
            UpdateStatus.Text = $"Не удалось открыть браузер: {ex.Message}";
        }
    }

    private void OnUpdatePage(object sender, RoutedEventArgs e) =>
        OpenReleasePage("Открыта страница выпуска.");

    private async void OnUpdateRetry(object sender, RoutedEventArgs e)
    {
        if (_downloading)
        {
            return;
        }
        UpdateStatus.Text = "Проверка обновлений…";
        var result = await CheckUpdateAsync();
        if (!result.Ok)
        {
            UpdateStatus.Text = result.Error;
            return;
        }
        ShowUpdate(result.Update, result.Installable);
        UpdateStatus.Text = result.Update is not null
            ? "Перед установкой ознакомьтесь с изменениями."
            : "У вас последняя версия.";
    }

    private void OnUpdateLater(object sender, RoutedEventArgs e) => UpdateFlyout.Hide();
}
