$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

if (-not $env:RUNNER_TOOL_CACHE -or -not $env:GITHUB_PATH) {
    throw 'RUNNER_TOOL_CACHE and GITHUB_PATH are required'
}
$version = (Get-Content -LiteralPath (Join-Path $PSScriptRoot '../.bun-version') -Raw).Trim()
if ($version -notmatch '^\d+\.\d+\.\d+$') {
    throw 'Expected an exact Bun version in .bun-version'
}

# A versioned runner-owned installation never replaces the developer's bun.exe.
$cacheParent = Join-Path $env:RUNNER_TOOL_CACHE 'anivox/bun'
$cachedBin = Join-Path $cacheParent $version
$cachedExecutable = Join-Path $cachedBin 'bun.exe'
if (Test-Path -LiteralPath $cachedExecutable) {
    $cachedVersion = & $cachedExecutable --version
    if ($LASTEXITCODE -ne 0 -or $cachedVersion.Trim() -ne $version) {
        throw "Invalid cached Bun installation: $cachedExecutable"
    }
    [System.IO.File]::AppendAllText($env:GITHUB_PATH, $cachedBin + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
    Write-Output "Reusing Bun $cachedVersion from $cachedExecutable"
    exit 0
}

New-Item -ItemType Directory -Path $cacheParent -Force | Out-Null
# Stage on the cache volume so publishing works even when RUNNER_TEMP is on
# another drive. Only the final version directory can be reused by future jobs.
$installDirectory = Join-Path $cacheParent ('.install-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $installDirectory | Out-Null
$archive = Join-Path $installDirectory 'bun.zip'
$downloadUrl = "https://github.com/oven-sh/bun/releases/download/bun-v$version/bun-windows-x64.zip"
Invoke-WebRequest -UseBasicParsing -Uri $downloadUrl -OutFile $archive -TimeoutSec 120
Expand-Archive -LiteralPath $archive -DestinationPath $installDirectory
$binDirectory = Join-Path $installDirectory 'bun-windows-x64'
$executable = Join-Path $binDirectory 'bun.exe'
$actualVersion = & $executable --version
if ($LASTEXITCODE -ne 0 -or $actualVersion.Trim() -ne $version) {
    throw "Bun version verification failed: expected $version, got $actualVersion"
}
# Publish only the verified, complete directory. A failed download is never reused.
[System.IO.Directory]::Move($binDirectory, $cachedBin)
Remove-Item -LiteralPath $archive
Remove-Item -LiteralPath $installDirectory
[System.IO.File]::AppendAllText($env:GITHUB_PATH, $cachedBin + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
Write-Output "Bun $actualVersion installed in runner cache: $cachedExecutable"
