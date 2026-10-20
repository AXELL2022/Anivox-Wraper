param([string]$Exe)

$ErrorActionPreference = 'Stop'
function Get-AnivoxSha256([string]$Path) {
    # Avoid depending on PowerShell module autoloading in background runners.
    $stream = [System.IO.File]::OpenRead($Path)
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
        return [System.BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '')
    } finally {
        $stream.Dispose()
        $algorithm.Dispose()
    }
}
if (-not $env:RUNNER_TEMP -or -not $env:GITHUB_ENV) {
    throw 'RUNNER_TEMP and GITHUB_ENV are required'
}
if (-not $Exe) {
    $Exe = if ($env:CARGO_TARGET_DIR) {
        Join-Path $env:CARGO_TARGET_DIR 'release/anivox.exe'
    } else {
        'src-tauri/target/release/anivox.exe'
    }
}
$productionExe = (Resolve-Path -LiteralPath $Exe).Path
$productionHash = Get-AnivoxSha256 $productionExe
$testDirectory = Join-Path $env:RUNNER_TEMP ('anivox-native-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory | Out-Null
$backup = Join-Path $testDirectory 'production.exe'
$testExe = Join-Path $testDirectory 'anivox.exe'
$config = Join-Path $testDirectory 'tauri-ci.json'
Copy-Item -LiteralPath $productionExe -Destination $backup
[System.IO.File]::WriteAllText($config, '{"identifier":"com.yasin.anivox.ci-native"}', [System.Text.UTF8Encoding]::new($false))
$previousTarget = $env:CARGO_TARGET_DIR
try {
    # Keep the alternate application identity from invalidating the production
    # target's Tauri fingerprints on every subsequent self-hosted build.
    if ($env:ANIVOX_CI_CACHE_ROOT) {
        $env:CARGO_TARGET_DIR = Join-Path $env:ANIVOX_CI_CACHE_ROOT 'native-target'
    }
    # Build only the test executable; signed installers have already been built.
    & bun run build --no-bundle --config $config
    if ($LASTEXITCODE -ne 0) { throw 'Failed to build the isolated native-test executable' }
    $builtExe = if ($env:ANIVOX_CI_CACHE_ROOT) {
        Join-Path $env:CARGO_TARGET_DIR 'release/anivox.exe'
    } else {
        $productionExe
    }
    Copy-Item -LiteralPath $builtExe -Destination $testExe
} finally {
    $env:CARGO_TARGET_DIR = $previousTarget
    # Upload and release steps must always receive the original production binary.
    Copy-Item -LiteralPath $backup -Destination $productionExe -Force
    if ((Get-AnivoxSha256 $productionExe) -ne $productionHash) {
        throw 'Production executable was not restored intact'
    }
}
[System.IO.File]::AppendAllText($env:GITHUB_ENV, "ANIVOX_NATIVE_TEST_EXE=$testExe" + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
Write-Output "Isolated native test executable: $testExe"
Write-Output "Production SHA256 preserved: $productionHash"
