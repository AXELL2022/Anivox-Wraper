# Builds the Anivox WinUI installer: dotnet publish (self-contained, win-x64) + NSIS.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File windows-winui/installer/build-installer.ps1
#
# Output: windows-winui/installer/out/Anivox_<version>_x64-setup.exe. Signing for the
# updater (`tauri signer sign`) is a separate release step. -ProductKey builds a test
# installer with its own uninstall key, folder and shortcut, so a real install is untouched.
param(
    [string]$OutDir = (Join-Path $PSScriptRoot 'out'),
    [string]$Makensis = '',
    [string]$ProductKey = ''
)
$ErrorActionPreference = 'Stop'

$winui = Split-Path $PSScriptRoot -Parent
$project = Join-Path $winui 'Anivox.WinUI.csproj'
$version = ([xml](Get-Content -LiteralPath $project -Raw)).Project.PropertyGroup.Version | Where-Object { $_ } | Select-Object -First 1
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw "Unexpected version in Anivox.WinUI.csproj: '$version'" }

if (-not $Makensis) {
    $candidates = @(
        (Get-Command makensis.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source),
        (Join-Path $env:LOCALAPPDATA 'tauri\NSIS\makensis.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe'),
        (Join-Path $env:ProgramFiles 'NSIS\makensis.exe')
    ) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }
    $Makensis = $candidates | Select-Object -First 1
}
if (-not $Makensis) { throw 'makensis.exe not found: install NSIS 3 or pass -Makensis' }

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$publish = Join-Path $OutDir 'publish'
if (Test-Path -LiteralPath $publish) { Remove-Item -LiteralPath $publish -Recurse -Force }

Write-Output "Publishing Anivox $version (win-x64)"
dotnet publish $project -c Release -r win-x64 -o $publish -nologo -v quiet
if ($LASTEXITCODE) { throw "dotnet publish failed ($LASTEXITCODE)" }
if (-not (Test-Path -LiteralPath (Join-Path $publish 'anivox-winui.exe'))) { throw 'anivox-winui.exe is missing in the publish output' }
if (-not (Test-Path -LiteralPath (Join-Path $publish 'anivox_ffi.dll'))) { throw 'anivox_ffi.dll is missing in the publish output' }
# Without resources.pri (EnableMsixTooling in the csproj) the app dies at startup.
if (-not (Test-Path -LiteralPath (Join-Path $publish 'anivox-winui.pri'))) { throw 'anivox-winui.pri is missing in the publish output' }

$name = if ($ProductKey) { "$ProductKey`_$version`_x64-setup.exe" } else { "Anivox_$version`_x64-setup.exe" }
$setup = Join-Path $OutDir $name
$defines = @("/DVERSION=$version", "/DSOURCE_DIR=$publish", "/DOUTFILE=$setup")
if ($ProductKey) { $defines += "/DPRODUCT_KEY=$ProductKey" }

Write-Output "Building $name with $Makensis"
& $Makensis /V2 /INPUTCHARSET UTF8 @defines (Join-Path $PSScriptRoot 'anivox.nsi')
if ($LASTEXITCODE) { throw "makensis failed ($LASTEXITCODE)" }
$size = [math]::Round((Get-Item -LiteralPath $setup).Length / 1MB, 1)
Write-Output "Installer: $setup ($size MB)"
