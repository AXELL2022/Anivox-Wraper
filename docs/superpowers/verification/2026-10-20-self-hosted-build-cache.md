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
- Full Rust speedup requires a warm CI run; no percentage speedup is claimed
  from the isolated Bun measurements. A cold target still compiles dependencies.
