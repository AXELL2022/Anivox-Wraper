$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

if (-not $env:RUNNER_TEMP -or -not $env:GITHUB_PATH) {
    throw 'RUNNER_TEMP and GITHUB_PATH are required'
}
$version = (Get-Content -LiteralPath (Join-Path $PSScriptRoot '../.bun-version') -Raw).Trim()
if ($version -notmatch '^\d+\.\d+\.\d+$') {
    throw 'Expected an exact Bun version in .bun-version'
}

# Never replace a developer's running bun.exe. Each job gets its own directory.
$installDirectory = Join-Path $env:RUNNER_TEMP ('anivox-bun-' + [Guid]::NewGuid().ToString('N'))
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
[System.IO.File]::AppendAllText($env:GITHUB_PATH, $binDirectory + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
Write-Output "Bun $actualVersion installed for this job: $executable"
