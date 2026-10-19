param([Parameter(Mandatory = $true)][string]$Exe)

$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class AnivoxSingleInstanceTest {
    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hwnd, int command);
    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hwnd, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")]
    public static extern bool IsIconic(IntPtr hwnd);
    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();
}
'@

$resolvedExe = (Resolve-Path -LiteralPath $Exe).Path
if (Get-Process -Name anivox -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $resolvedExe }) {
    throw 'Close existing instances of the executable under test before running this test'
}
$testProcesses = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
try {
    $first = Start-Process -FilePath $resolvedExe -PassThru -WindowStyle Hidden
    $testProcesses.Add($first)
    $deadline = [DateTime]::UtcNow.AddSeconds(25)
    do {
        Start-Sleep -Milliseconds 150
        $first.Refresh()
        if ($first.HasExited) { throw 'First instance exited before creating its window' }
    } while ($first.MainWindowHandle -eq 0 -and [DateTime]::UtcNow -lt $deadline)
    $hwnd = $first.MainWindowHandle
    if ($hwnd -eq 0) { throw 'No application window was found' }

    foreach ($state in @('visible', 'minimized', 'hidden')) {
        if ($state -eq 'minimized') {
            [void][AnivoxSingleInstanceTest]::ShowWindow($hwnd, 6)
            if (-not [AnivoxSingleInstanceTest]::IsIconic($hwnd)) {
                throw 'Could not minimize the test window'
            }
        } elseif ($state -eq 'hidden') {
            # Follow the real CloseRequested -> window.hide() path so Tauri's
            # visibility state matches the native window state.
            if (-not [AnivoxSingleInstanceTest]::PostMessage($hwnd, 0x10, [IntPtr]::Zero, [IntPtr]::Zero)) {
                throw 'Could not send the close request'
            }
            $deadline = [DateTime]::UtcNow.AddSeconds(5)
            while ([AnivoxSingleInstanceTest]::IsWindowVisible($hwnd) -and [DateTime]::UtcNow -lt $deadline) {
                Start-Sleep -Milliseconds 100
            }
            if ([AnivoxSingleInstanceTest]::IsWindowVisible($hwnd)) {
                throw 'Could not hide the test window'
            }
        }

        $second = Start-Process -FilePath $resolvedExe -PassThru -WindowStyle Hidden
        $testProcesses.Add($second)
        if (-not $second.WaitForExit(5000)) {
            throw "Second instance remained running with the first window $state"
        }
        if ($second.ExitCode -ne 0) { throw "Second instance failed: $($second.ExitCode)" }

        $deadline = [DateTime]::UtcNow.AddSeconds(5)
        do {
            $restored = [AnivoxSingleInstanceTest]::IsWindowVisible($hwnd) -and
                -not [AnivoxSingleInstanceTest]::IsIconic($hwnd) -and
                [AnivoxSingleInstanceTest]::GetForegroundWindow() -eq $hwnd
            if (-not $restored) { Start-Sleep -Milliseconds 100 }
        } while (-not $restored -and [DateTime]::UtcNow -lt $deadline)
        $first.Refresh()
        if ($first.HasExited) { throw 'Original instance exited during relaunch' }
        if (-not $restored) {
            $visible = [AnivoxSingleInstanceTest]::IsWindowVisible($hwnd)
            $minimized = [AnivoxSingleInstanceTest]::IsIconic($hwnd)
            $foreground = [AnivoxSingleInstanceTest]::GetForegroundWindow()
            throw "Original $state window was not restored: visible=$visible minimized=$minimized foreground=$foreground expected=$hwnd"
        }
        Write-Output "Relaunch with $state window: OK"
    }
} finally {
    # Only stop processes launched by this test.
    foreach ($testProcess in $testProcesses) {
        if (-not $testProcess.HasExited) { Stop-Process -Id $testProcess.Id -Force }
        $testProcess.Dispose()
    }
}
