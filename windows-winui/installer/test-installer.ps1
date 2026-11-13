# Installer checks against a throwaway product key, so a real Anivox install is never touched:
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File windows-winui/installer/test-installer.ps1
#
# Builds a test installer (build-installer.ps1 -ProductKey AnivoxInstallerTest) and a stand-in
# for the Tauri uninstaller, then checks: the Tauri updater run over a Tauri install
# (/P /UPDATE /R /ARGS), the WinUI self-update (/P /UPDATE), silent uninstall and a fresh silent
# install. User data (%APPDATA%\com.yasin.anivox) must survive. Close Anivox test windows it opens
# only through this script; it stops the instances it started.
param([string]$Makensis = '')
$ErrorActionPreference = 'Stop'
$key = 'AnivoxInstallerTest'
$work = Join-Path ([IO.Path]::GetTempPath()) 'anivox-installer-test'
New-Item -ItemType Directory -Force $work | Out-Null

$buildArgs = @{ OutDir = $work; ProductKey = $key }
if ($Makensis) { $buildArgs.Makensis = $Makensis }
& (Join-Path $PSScriptRoot 'build-installer.ps1') @buildArgs | Out-Host
$setup = Get-ChildItem $work -Filter "$($key)_*_x64-setup.exe" | Sort-Object LastWriteTime | Select-Object -Last 1 -ExpandProperty FullName
if (-not $Makensis) {
    $Makensis = @((Get-Command makensis.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source),
        (Join-Path $env:LOCALAPPDATA 'tauri\NSIS\makensis.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe')) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
}

# Stand-in for the Tauri uninstaller: logs its command line, removes its exe and key.
$fakeScript = Join-Path $work 'fake-tauri-uninstall.nsi'
@"
Unicode true
RequestExecutionLevel user
SilentInstall silent
OutFile "fake-uninstall.exe"
Section
  FileOpen `$0 "`$TEMP\anivox-fake-uninstall.log" a
  FileWrite `$0 "`$CMDLINE`$\r`$\n"
  FileClose `$0
  Delete "`$EXEDIR\anivox.exe"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\$key"
SectionEnd
"@ | Set-Content -LiteralPath $fakeScript -Encoding UTF8
Push-Location $work
try { & $Makensis /V1 $fakeScript; if ($LASTEXITCODE) { throw 'fake uninstaller build failed' } } finally { Pop-Location }
$here = $work
$version = ([xml](Get-Content -LiteralPath (Join-Path (Split-Path $PSScriptRoot -Parent) "Anivox.WinUI.csproj") -Raw)).Project.PropertyGroup.Version | Where-Object { $_ } | Select-Object -First 1

$dir = Join-Path $env:LOCALAPPDATA $key
$reg = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$key"
$manu = "HKCU:\Software\yasin\$key"
$startMenu = Join-Path ([Environment]::GetFolderPath('Programs')) "$key.lnk"
$desktop = Join-Path ([Environment]::GetFolderPath('Desktop')) "$key.lnk"
$log = Join-Path $env:TEMP 'anivox-fake-uninstall.log'
$data = Join-Path $env:APPDATA 'com.yasin.anivox'
$shell = New-Object -ComObject WScript.Shell
$failures = 0
function Check([string]$what, [bool]$ok) {
    if ($ok) { "  ok   $what" } else { "  FAIL $what"; $script:failures++ }
}
function Target($lnk) { if (Test-Path $lnk) { $shell.CreateShortcut($lnk).TargetPath } else { '' } }
function Stop-TestApp {
    Get-Process anivox-winui -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$dir*" } | Stop-Process -Force
}
# Start-Process -Wait would also wait for the Anivox instance the installer starts.
function Run-Setup([string]$arguments) {
    $p = [System.Diagnostics.Process]::Start($setup, $arguments)
    $p.WaitForExit()
    $p.ExitCode
}
function Run-Uninstall {
    $p = [System.Diagnostics.Process]::Start("$dir\uninstall.exe", '/S')
    $p.WaitForExit()
    for ($i = 0; $i -lt 40 -and (Test-Path $dir); $i++) { Start-Sleep -Milliseconds 500 }
}
foreach ($guard in @($dir, $startMenu, $desktop)) { if (Test-Path $guard) { throw "leftover from an earlier run: $guard" } }
if (Test-Path $reg) { throw "leftover registry key $reg" }
$dataBefore = Test-Path $data

"== 1. Tauri updater run over a Tauri-style install (/P /UPDATE /R /ARGS)"
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item (Join-Path $here 'fake-uninstall.exe') (Join-Path $dir 'uninstall.exe')
Set-Content (Join-Path $dir 'anivox.exe') 'old tauri binary'
New-Item -Force $reg | Out-Null
Set-ItemProperty $reg DisplayName 'Anivox'
Set-ItemProperty $reg DisplayVersion '0.4.3'
Set-ItemProperty $reg InstallLocation "`"$dir`""
Set-ItemProperty $reg UninstallString "`"$dir\uninstall.exe`""
New-Item -Force $manu | Out-Null
Set-ItemProperty $manu '(default)' $dir
$lnk = $shell.CreateShortcut($startMenu); $lnk.TargetPath = "$dir\anivox.exe"; $lnk.Save()
Remove-Item $log -ErrorAction SilentlyContinue

$code = Run-Setup '/P /UPDATE /R /ARGS'
Check "installer exit code 0 ($code)" ($code -eq 0)
$args1 = if (Test-Path $log) { (Get-Content $log) -join ' | ' } else { '' }
Check "old uninstaller ran with /UPDATE /P _?=dir ($args1)" ($args1 -match [regex]::Escape("/UPDATE /P _?=$dir"))
Check 'old anivox.exe removed' (-not (Test-Path "$dir\anivox.exe"))
Check 'anivox-winui.exe installed' (Test-Path "$dir\anivox-winui.exe")
Check 'anivox_ffi.dll installed' (Test-Path "$dir\anivox_ffi.dll")
Check 'no .pdb shipped' (-not (Get-ChildItem $dir -Filter *.pdb -Recurse))
Check 'own uninstaller written' ((Get-Item "$dir\uninstall.exe").Length -gt 50000)
$props = Get-ItemProperty $reg
Check "uninstall key marked winui ($($props.AnivoxShell))" ($props.AnivoxShell -eq 'winui')
Check "installed version $version ($($props.DisplayVersion))" ($props.DisplayVersion -eq $version)
Check 'Tauri install-location key removed' (-not (Test-Path $manu))
Check "start menu shortcut points to the new exe ($(Target $startMenu))" ((Target $startMenu) -eq "$dir\anivox-winui.exe")
Check 'no desktop shortcut added on an update' (-not (Test-Path $desktop))
Start-Sleep 5
$app = Get-Process anivox-winui -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$dir*" }
Check 'Anivox restarted after the update (/R)' ([bool]$app)
Stop-TestApp
Start-Sleep 1

"== 2. WinUI self-update over the WinUI install (/P /UPDATE, app running)"
Start-Process "$dir\anivox-winui.exe" | Out-Null
Start-Sleep 4
Set-Content "$dir\stale-file.txt" 'from an older version'
Remove-Item $log -ErrorAction SilentlyContinue
$code = Run-Setup '/P /UPDATE'
Check "installer exit code 0 ($code)" ($code -eq 0)
Check 'Tauri uninstaller not called for a WinUI install' (-not (Test-Path $log))
Check 'stale files of the previous version removed' (-not (Test-Path "$dir\stale-file.txt"))
Check 'anivox-winui.exe present' (Test-Path "$dir\anivox-winui.exe")
Start-Sleep 5
$app = Get-Process anivox-winui -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$dir*" }
Check 'Anivox restarted after the self-update' ([bool]$app)
Stop-TestApp
Start-Sleep 1

"== 3. Silent uninstall"
Run-Uninstall
Check 'install folder removed' (-not (Test-Path $dir))
Check 'uninstall key removed' (-not (Test-Path $reg))
Check 'start menu shortcut removed' (-not (Test-Path $startMenu))
Check 'user data kept' ((Test-Path $data) -eq $dataBefore)

"== 4. Fresh silent install (/S): no relaunch, desktop shortcut on first install"
$code = Run-Setup '/S'
Check "installer exit code 0 ($code)" ($code -eq 0)
Check 'installed' (Test-Path "$dir\anivox-winui.exe")
Check "desktop shortcut created ($(Target $desktop))" ((Target $desktop) -eq "$dir\anivox-winui.exe")
Start-Sleep 3
Check 'not started without /R or /UPDATE' (-not (Get-Process anivox-winui -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$dir*" }))
Run-Uninstall
Check 'removed again, desktop shortcut too' (-not (Test-Path $dir) -and -not (Test-Path $desktop) -and -not (Test-Path $reg))

Stop-TestApp
"failures: $failures"
if ($failures) { exit 1 }
