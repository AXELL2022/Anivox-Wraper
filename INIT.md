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
├── src-tauri/
│   ├── Cargo.toml               # Rust dependencies (tauri v2, discord-rich-presence, serde)
│   ├── tauri.conf.json          # Tauri v2 configuration (product name, identifier, bundle icons)
│   ├── capabilities/
│   │   └── default.json         # Permissions for remote domain (https://anivox.fun/*)
│   ├── permissions/
│   │   └── discord.toml         # Custom IPC permission: allow-set-discord-rpc
│   └── src/
│       ├── main.rs              # Entry point: Linux WebKitGTK Wayland fix + calls anivox_lib::run()
│       └── lib.rs               # Core logic: Webview window setup, JS injection, Discord RPC
├── src/                         # Local vanilla fallback/assets (app opens remote URL directly)
├── package.json                 # Node scripts (tauri dev, tauri build)
├── GEMINI.md                    # Antigravity/Gemini system rules file (auto-loaded on every prompt)
└── INIT.md                      # Quick reference project summary for AI sessions
```

---

## 3. Key Components & Implementation Details

### 3.1. Main Window & Navigation Injection (`src-tauri/src/lib.rs`)
* The app opens `tauri::WebviewUrl::External("https://anivox.fun/".parse().unwrap())`.
* **Injected Header (`#tauri-header`):** A fixed 32px top bar with **Back** (`window.history.back()`), **Forward** (`window.history.forward()`), and **Reload** (`window.location.reload()`) buttons. It also offsets `body { margin-top: 32px !important; }`.
* **Discord RPC Bridge:** A `MutationObserver` on `document.querySelector('title')` triggers `window.__TAURI__.core.invoke('set_discord_rpc', { ... })` whenever the page changes, stripping the `" - Anivox"` suffix from the title. Also polls every 30 seconds.

### 3.2. WireGuard VPN Integration (`src-tauri/src/lib.rs`)
* Config file: `ANIVOX-UA-48.conf` (embedded via `include_str!` and checked locally).
* IPC Commands: `get_vpn_status()` and `toggle_vpn(enable: Option<bool>)`.
* Linux: Uses `nmcli connection up/down ANIVOX-UA-48`, automatic import if not present, with `wg-quick` fallback.
* Windows: Uses `wireguard.exe /installtunnelservice` and `/uninstalltunnelservice`.
* Injected Header Switch: Interactive toggle pill with green (active) / dark (inactive) states, loading indicator, and tooltips.

### 3.3. Discord Rich Presence (`src-tauri/src/lib.rs`)
* Uses `discord-rich-presence = "0.2.4"`.
* Application Client ID: `1504862803335315609`.
* Stored in Tauri managed state: `DiscordState(pub Mutex<Option<DiscordIpcClient>>)`.
* IPC Command: `set_discord_rpc(state, details, state_text)`.

### 3.4. Tauri v2 Remote Security & Capabilities
* Since the window loads a remote URL (`https://anivox.fun/*`), Tauri v2 requires explicit capability grants for remote IPC.
* Configured in `src-tauri/capabilities/default.json` referencing `remote.urls: ["https://anivox.fun/*"]`.
* Allowed permissions: `core:default`, `opener:default`, `allow-set-discord-rpc`, `allow-vpn-commands`.
* If adding new Rust IPC commands, remember to define permissions in `src-tauri/permissions/` and add them to `capabilities/default.json`.

---

## 4. Platform-Specific Quirks & Environments

### 4.1. Linux (Arch / CachyOS / Wayland / Hyprland)
* **Rendering Engine:** WebKit2GTK 4.1 (`wry`).
* **Wayland DMA-BUF & Tearing Fix:** In `src-tauri/src/main.rs`, `WEBKIT_DISABLE_DMABUF_RENDERER=1` is set on Linux. This avoids the Wayland protocol crash (Error 71) and prevents white horizontal tearing lines during scrolling on NVIDIA GPUs.
* **Browser Arguments:** Chromium/Edge flags (e.g. `--use-angle=d3d11`, `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`) are Windows WebView2 specific and ignored by WebKitGTK.

### 4.2. Windows
* **Rendering Engine:** Microsoft Edge WebView2.
* Uses custom hardware overlay & video upscaling arguments passed via `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`.

---

## 5. Standard Commands

* **Dev Run:** `npm run tauri dev`
* **Production Build:** `npm run tauri build`
* **Rust Fast Check:** `cargo check --manifest-path src-tauri/Cargo.toml`
* **Tauri Environment Info:** `npx tauri info`

---

## 6. Conventions for AI Modifications
* **Backend Logic:** Keep application logic in `src-tauri/src/lib.rs`. Keep `src-tauri/src/main.rs` as a minimal entry point.
* **Remote Injections:** Any UI modifications or client-side scripts to be injected into the remote website should be placed in the `script` string literal within `src-tauri/src/lib.rs`.
* **Security / Permissions:** Do not bypass Tauri v2 capabilities. When adding new IPC commands, update both the `permissions/*.toml` and `capabilities/*.json` files.
