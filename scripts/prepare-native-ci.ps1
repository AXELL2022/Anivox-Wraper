param([string]$Exe = 'src-tauri/target/release/anivox.exe')

$ErrorActionPreference = 'Stop'
if (-not $env:RUNNER_TEMP -or -not $env:GITHUB_ENV) {
    throw 'RUNNER_TEMP and GITHUB_ENV are required'
}
$productionExe = (Resolve-Path -LiteralPath $Exe).Path
$productionHash = (Get-FileHash -LiteralPath $productionExe -Algorithm SHA256).Hash
$testDirectory = Join-Path $env:RUNNER_TEMP ('anivox-native-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory | Out-Null
$backup = Join-Path $testDirectory 'production.exe'
$testExe = Join-Path $testDirectory 'anivox.exe'
$config = Join-Path $testDirectory 'tauri-ci.json'
Copy-Item -LiteralPath $productionExe -Destination $backup
[System.IO.File]::WriteAllText($config, '{"identifier":"com.yasin.anivox.ci-native"}', [System.Text.UTF8Encoding]::new($false))
try {
    # Build only the test executable; signed installers have already been built.
    & bun run build --no-bundle --config $config
    if ($LASTEXITCODE -ne 0) { throw 'Failed to build the isolated native-test executable' }
    Copy-Item -LiteralPath $productionExe -Destination $testExe
} finally {
    # Upload and release steps must always receive the original production binary.
    Copy-Item -LiteralPath $backup -Destination $productionExe -Force
    if ((Get-FileHash -LiteralPath $productionExe -Algorithm SHA256).Hash -ne $productionHash) {
        throw 'Production executable was not restored intact'
    }
}
[System.IO.File]::AppendAllText($env:GITHUB_ENV, "ANIVOX_NATIVE_TEST_EXE=$testExe" + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
Write-Output "Isolated native test executable: $testExe"
Write-Output "Production SHA256 preserved: $productionHash"
