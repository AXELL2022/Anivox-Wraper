# Self-hosted Windows build cache

Baseline: release workflow 39462927144 on AXELL, Ryzen 7 7840HS (8 cores,
16 threads), 31 GiB RAM. Rust tests: 69 s; cargo check: 46 s; production
Tauri build: 148 s; alternate-identity native EXE: 53 s; Bun setup: 16 s.
The listener uses Normal priority and affinity 65535 (all 16 logical CPUs).

Changes:

- Production CARGO_TARGET_DIR and native-test target are persistent directories
  under RUNNER_TOOL_CACHE/anivox, outside checkout's git clean scope.
- Windows skips rust-cache restore/prune/upload. Linux retains cloud caching.
- Cargo uses all available logical CPUs. Dev/test debug symbols are disabled
  only in CI; production optimization settings are unchanged.
- Only old release/bundle installers are removed; compiled dependencies remain.
- Native-test preparation honors the external target and restores the previous
  environment and original production EXE/hash even after failure.
- SHA-256 uses .NET directly: workflow 39464325475 reached signed bundles but
  its background PowerShell failed to autoload Get-FileHash before native tests.
- Bun is verified before publication into a versioned runner-owned directory.
  Staging and atomic publication use the same volume. Existing developer Bun
  installations are never overwritten.

Checks:

- Actual Bun download and subsequent cache reuse passed (first probe 8.1 s
  versus 1.45 s; final implementation 9.48 s versus 2.71 s).
- Setup retained a dependency marker and removed a stale installer marker.
- A controlled build-command harness verified native artifact selection,
  production integrity and target environment restoration on success/failure.
- PowerShell parsing and all 87 JavaScript tests passed.

Completed Actions verification:

- Release run 39465779988: Windows/Linux builds, native icons, visibility
  restoration and publication all succeeded. v0.3.1 has 12 release assets;
  latest.json has both updater platforms and identical release notes.
- Warm run 39466592682: Windows job succeeded in 161 s; Linux in 273 s.
  No release was republished by this workflow_dispatch run.

| Windows step | Baseline (s) | Warm cache (s) |
| --- | ---: | ---: |
| Bun setup | 16 | 2 |
| Rust tests | 69 | 8 |
| cargo check | 46 | 1 |
| Tauri application + installers | 148 | 64 |
| Alternate-identity native EXE | 53 | 51 |

The native EXE still spends about 51 s in preparation; it is not claimed to be
instant. The measurements above cover one warm run on the same Windows runner,
not a guarantee for clean builds, new dependencies or a changed toolchain.
