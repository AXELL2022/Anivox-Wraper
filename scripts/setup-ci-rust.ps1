$ErrorActionPreference = 'Stop'
if (-not $env:RUNNER_TOOL_CACHE -or -not $env:GITHUB_ENV) {
    throw 'RUNNER_TOOL_CACHE and GITHUB_ENV are required'
}

# checkout cleans ignored files inside the repository. Keep the self-hosted
# runner's complete build output outside it, without touching developer targets.
$cacheRoot = Join-Path $env:RUNNER_TOOL_CACHE 'anivox'
$targetDirectory = Join-Path $cacheRoot 'target'
New-Item -ItemType Directory -Path $targetDirectory -Force | Out-Null
# Keep compiled dependencies, but never upload installers from an older release.
$targetFullPath = [System.IO.Path]::GetFullPath($targetDirectory)
$bundleDirectory = [System.IO.Path]::GetFullPath((Join-Path $targetDirectory 'release/bundle'))
if (-not $bundleDirectory.StartsWith($targetFullPath + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Bundle cleanup escaped the persistent target directory'
}
if (Test-Path -LiteralPath $bundleDirectory) {
    Remove-Item -LiteralPath $bundleDirectory -Recurse -Force
}
$jobs = [Environment]::ProcessorCount
$values = @(
    "ANIVOX_CI_CACHE_ROOT=$cacheRoot",
    "CARGO_TARGET_DIR=$targetDirectory",
    "CARGO_BUILD_JOBS=$jobs",
    'CARGO_PROFILE_DEV_DEBUG=0',
    'CARGO_PROFILE_TEST_DEBUG=0'
)
[System.IO.File]::AppendAllText($env:GITHUB_ENV, ($values -join [Environment]::NewLine) + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
Write-Output "Persistent Cargo target: $targetDirectory"
Write-Output "Cargo parallel jobs: $jobs; development/test debug symbols disabled for CI"
