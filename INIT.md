# Anivox Desktop — AI Assistant Context & Project Knowledge Base

## 1. Project Overview
* **Product Name:** Anivox Desktop (`com.yasin.anivox`)
* **Target Site:** `https://anivox.fun/` (Remote external URL)
* **Framework:** Tauri v2 (Rust backend + remote webview wrapper)
* **Primary Platform:** Linux (Arch / CachyOS, Hyprland / Wayland) & Windows cross-compatibility

---

## 2. Architecture & File Structure

```
.
├── PKGBUILD                     # Native Arch Linux package build script
├── CHANGELOG.md                 # Complete historical changelog of all iterations
├── ANIVOX-UA-48.conf            # WireGuard configuration file
├── src-tauri/
│   ├── Cargo.toml               # Rust dependencies (tauri v2, discord-rich-presence, serde)
│   ├── tauri.conf.json          # Tauri v2 configuration (product name, identifier, bundle icons)
│   ├── capabilities/
│   │   └── default.json         # Permissions for remote domain (https://anivox.fun/*)
│   ├── permissions/
│   │   ├── discord.toml         # Custom IPC permission: allow-set-discord-rpc
│   │   └── vpn.toml             # Custom IPC permission: allow-vpn-commands
│   ├── binaries/
│   │   └── wireproxy            # High-speed userspace WireGuard SOCKS5 binary (embedded)
│   └── src/
│       ├── main.rs              # Entry point: Linux WebKitGTK Wayland fix + calls anivox_lib::run()
│       ├── lib.rs               # Core logic: Webview window setup, JS injection, Discord RPC
│       └── proxy.rs             # SmartProxy: process-isolated userspace WireGuard SOCKS5 engine
├── src/                         # Local vanilla fallback/assets (app opens remote URL directly)
├── package.json                 # Node scripts (tauri dev, build, build:arch, install:arch)
├── GEMINI.md                    # Antigravity/Gemini system rules file (auto-loaded on every prompt)
└── INIT.md                      # Quick reference project summary for AI sessions
```

---

## 3. Key Components & Implementation Details

### 3.1. Main Window & Navigation Injection (`src-tauri/src/lib.rs`)
* The app opens `tauri::WebviewUrl::External("https://anivox.fun/".parse().unwrap())`.
* **Injected Header (`#tauri-header`):** A fixed 32px top bar with **Back** (`window.history.back()`), **Forward** (`window.history.forward()`), and **Reload** (`window.location.reload()`) buttons. It also offsets `body { margin-top: 32px !important; }`.
* **Discord RPC Bridge:** A `MutationObserver` on `document.querySelector('title')` triggers `window.__TAURI__.core.invoke('set_discord_rpc', { ... })` whenever the page changes, stripping the `" - Anivox"` suffix from the title. Also polls every 30 seconds.

### 3.2. WireGuard In-App VPN Integration (`src-tauri/src/proxy.rs` & `src-tauri/src/lib.rs`)
* Config file: `ANIVOX-UA-48.conf` (parsed dynamically).
* Architecture: Embedded userspace WireGuard SOCKS5 engine (`SmartProxy`) powered by native `wireproxy` (`wireguard-go`), embedded directly into the binary via `include_bytes!`.
* Window Binding: Attached via Tauri's `.proxy_url("socks5://127.0.0.1:10808")`.
* IPC Commands: `get_vpn_status()` and `toggle_vpn(enable: Option<bool>)` (instantaneous atomic switch).
* Default State: VPN is enabled by default on launch (`is_vpn_enabled = true`).
* Performance: Native gigabit throughput with multi-connection parallel streaming (0.3–0.4s per asset), `TCP_NODELAY` enabled on sockets.
* Video Mode: Injected CSS automatically hides `#tauri-header` when watching anime full-screen (`:fullscreen` / `:-webkit-full-screen`).
* **System Isolation:** Completely process-isolated. Does NOT create system network adapters, does NOT call `nmcli`, does NOT alter system routes or DNS. Other applications (Discord, Telegram, Steam, browsers) remain on normal home internet without interruption.

### 3.3. Discord Rich Presence (`src-tauri/src/lib.rs`)
* Uses `discord-rich-presence = "0.2.4"`.
* Application Client ID: `1504862803335315609`.
* Stored in Tauri managed state: `DiscordState(pub Mutex<Option<DiscordIpcClient>>)`.
* IPC Command: `set_discord_rpc(state, details, state_text)`.

### 3.4. Tauri v2 Remote Security & Capabilities
* Since the window loads a remote URL (`https://anivox.fun/*`), Tauri v2 requires explicit capability grants for remote IPC.
* Configured in `src-tauri/capabilities/default.json` referencing `remote.urls: ["https://anivox.fun/*"]`.
* Allowed permissions: `core:default`, `opener:default`, `allow-set-discord-rpc`, `allow-vpn-commands`.
* Guards in injected script ensure IPC and topbar only execute on `https://anivox.fun/*` and never on `about:blank`.

---

## 4. Platform-Specific Quirks & Environments

### 4.1. Linux (Arch / CachyOS / Wayland / Hyprland)
* **Rendering Engine:** WebKit2GTK 4.1 (`wry`).
* **Wayland DMA-BUF & Tearing Fix:** In `src-tauri/src/main.rs`, `WEBKIT_DISABLE_DMABUF_RENDERER=1` is set on Linux. This avoids the Wayland protocol crash (Error 71) and prevents white horizontal tearing lines during scrolling on NVIDIA GPUs.
* **Packaging:** Built natively for Arch Linux via `PKGBUILD` into `.pkg.tar.zst`.

### 4.2. Windows
* **Rendering Engine:** Microsoft Edge WebView2.
* Uses custom hardware overlay & video upscaling arguments passed via `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`.

---

## 5. Standard Commands

* **Dev Run:** `npm run tauri dev`
* **Native Arch Linux Build:** `npm run build:arch` (produces `anivox-0.1.0-1-x86_64.pkg.tar.zst`)
* **Install on Arch Linux:** `npm run install:arch` (or `sudo pacman -U anivox-0.1.0-1-x86_64.pkg.tar.zst`)
* **Standard Universal Build:** `npm run build` (produces AppImage, deb, rpm)
* **Rust Fast Check:** `cargo check --manifest-path src-tauri/Cargo.toml`
* **Tauri Environment Info:** `npx tauri info`

---

## 6. Conventions for AI Modifications
* **Backend Logic:** Keep application logic in `src-tauri/src/lib.rs` and proxy routing in `src-tauri/src/proxy.rs`. Keep `src-tauri/src/main.rs` as a minimal entry point.
* **Remote Injections:** Any UI modifications or client-side scripts to be injected into the remote website should be placed in the `script` string literal within `src-tauri/src/lib.rs`.
* **Security / Permissions:** Do not bypass Tauri v2 capabilities. When adding new IPC commands, update both the `permissions/*.toml` and `capabilities/*.json` files.

---

## 7. Project Changelog & Evolution History
Detailed changelog is documented in [CHANGELOG.md](file:///run/media/axell/3262618D6261571F/Anivox-Wraper/CHANGELOG.md). Summary of milestones:
* **[0.1.0] Project Init:** Basic Tauri v2 wrapper for `https://anivox.fun/` with injected navigation buttons and Discord RPC.
* **Iter 1 (WireGuard VPN):** Added UI toggle in topbar and NetworkManager (`nmcli`) integration.
* **Iter 2 (NVIDIA Wayland Fix):** Set `WEBKIT_DISABLE_DMABUF_RENDERER=1` to eliminate white horizontal stripes/tearing on NVIDIA.
* **Iter 3 (Arch AppImage Fix):** Added `NO_STRIP=1` to prevent `linuxdeploy` crash on `.relr.dyn`.
* **Iter 4 (System Isolation):** Replaced system `nmcli` with in-app userspace SOCKS5 proxy to prevent whole-system VPN routing and DNS breakdown on disconnect.
* **Iter 5 (Origin Fix):** Prevented script execution on `about:blank` to fix `Origin header is not a valid URL` error.
* **Iter 6 (Default VPN State):** Set VPN enabled by default on app launch.
* **Iter 7 (DoH Resolver):** Added DNS over HTTPS (DoH) to bypass ISP `SERVFAIL` blocking on `anivox.fun` domain and eliminate `Operation was cancelled`.
* **Iter 8 (Wireproxy Engine & Speed):** Replaced slow `smoltcp` stack with high-speed embedded `wireproxy` (`wireguard-go`), fixed process drop, enabled `TCP_NODELAY`, and added fullscreen video topbar hiding.
* **Iter 9 (Native Arch Packaging):** Created `PKGBUILD` and `npm run build:arch` / `npm run install:arch` scripts producing native `anivox-0.1.0-1-x86_64.pkg.tar.zst` packages.
