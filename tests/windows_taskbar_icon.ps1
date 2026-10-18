param([Parameter(Mandatory = $true)][string]$Exe)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class AnivoxTaskbarIconTest {
    [DllImport("user32.dll", SetLastError=true)]
    public static extern IntPtr SendMessageTimeout(IntPtr hwnd, uint msg, IntPtr wp, IntPtr lp, uint flags, uint timeout, out IntPtr result);
}
'@

$resolvedExe = (Resolve-Path -LiteralPath $Exe).Path
$testProcess = Start-Process -FilePath $resolvedExe -PassThru -WindowStyle Hidden
try {
    $deadline = [DateTime]::UtcNow.AddSeconds(25)
    do {
        Start-Sleep -Milliseconds 150
        $testProcess.Refresh()
        if ($testProcess.HasExited) { throw 'Application exited before creating its window' }
    } while ($testProcess.MainWindowHandle -eq 0 -and [DateTime]::UtcNow -lt $deadline)
    $hwnd = $testProcess.MainWindowHandle
    if ($hwnd -eq 0) { throw 'No application window was found' }

    foreach ($entry in @(@('ICON_SMALL', 0), @('ICON_BIG', 1))) {
        $icon = [IntPtr]::Zero
        do {
            $testProcess.Refresh()
            if ($testProcess.HasExited) { throw 'Application exited during icon initialization' }
            $ok = [AnivoxTaskbarIconTest]::SendMessageTimeout($hwnd, 0x7f, [IntPtr]$entry[1], [IntPtr]::Zero, 2, 2000, [ref]$icon)
            if ($ok -eq 0) { throw 'Window did not respond to icon query' }
            if ($icon -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 150 }
        } while ($icon -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
        if ($icon -eq [IntPtr]::Zero) { throw "Window has no explicit $($entry[0]) icon" }
        # A nonzero handle must also refer to an intact image owned by the window.
        $bitmap = [System.Drawing.Icon]::FromHandle($icon).ToBitmap()
        try {
            if ($bitmap.Width -le 0 -or $bitmap.Height -le 0) { throw 'Invalid icon bitmap' }
        } finally { $bitmap.Dispose() }
    }
    Write-Output 'Window and taskbar icons: OK'
} finally {
    # Only terminate the instance created by this test, never an existing user session.
    if (-not $testProcess.HasExited) { Stop-Process -Id $testProcess.Id -Force }
}
