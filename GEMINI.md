# Project: Anivox Desktop (Tauri Web Wrapper)

## Project Overview
This project is a desktop application wrapper for the website `https://anivox.fun/`, built using the **Tauri v2** framework. It uses Rust for the backend and opens the remote website directly in a Webview window with custom UI injection and Discord Rich Presence support.

## Key Technologies & Environment
- **Framework:** Tauri v2
- **Backend:** Rust (`anivox_lib` in `src-tauri/src/lib.rs`, entrypoint in `src-tauri/src/main.rs`)
- **Remote Site:** `https://anivox.fun/`
- **Target OS:** Linux (Arch/CachyOS with Wayland/Hyprland) & Windows
- **Package Manager:** npm

## Project Architecture & Structure
```
├── src-tauri/
│   ├── Cargo.toml               # Dependencies: tauri v2, discord-rich-presence, serde
│   ├── tauri.conf.json          # App identifier (com.yasin.anivox), product name, bundle settings
│   ├── capabilities/default.json# Remote permissions for https://anivox.fun/*
│   ├── permissions/
│   │   ├── discord.toml         # Custom IPC permissions (allow-set-discord-rpc)
│   │   └── vpn.toml             # Custom IPC permissions (allow-vpn-commands)
│   └── src/
│       ├── main.rs              # Main entrypoint: sets WEBKIT_DISABLE_DMABUF_RENDERER=1 on Linux
│       └── lib.rs               # App core: window creation, injected topbar, Discord RPC, WireGuard VPN
├── ANIVOX-UA-48.conf            # WireGuard configuration file
├── src/                         # Local vanilla HTML/JS fallback assets
├── package.json                 # npm scripts
└── INIT.md                      # Detailed context reference file
```

## Key Mechanisms
1. **Remote Navigation & UI Injection (`src-tauri/src/lib.rs`):**
   - The webview loads `tauri::WebviewUrl::External("https://anivox.fun/".parse().unwrap())`.
   - A custom topbar (`#tauri-header`) is injected with Back, Forward, Reload buttons, and a WireGuard VPN toggle.
2. **Isolated In-App WireGuard VPN Toggle (`ANIVOX-UA-48`):**
   - Implemented via high-speed userspace WireGuard SOCKS5 engine (`SmartProxy` in `src-tauri/src/proxy.rs` using embedded `wireproxy`).
   - Runs on local port (e.g. `127.0.0.1:10808`), attached to the webview via `.proxy_url()`.
   - **Zero system impact**: No network interfaces, no `nmcli`, no routes/DNS changes, other apps (Discord, Telegram, Steam) never touch the VPN and system internet never disconnects.
   - Controlled via `get_vpn_status` and `toggle_vpn` IPC commands (atomic boolean in memory).
3. **Discord Rich Presence:**
   - Managed via `DiscordState` and the `discord-rich-presence` crate (App ID `1504862803335315609`).
   - Injected script observes document title mutations and invokes `set_discord_rpc`.
4. **Linux / Wayland Compatibility:**
   - In `src-tauri/src/main.rs`, `WEBKIT_DISABLE_DMABUF_RENDERER=1` is set to avoid WebKitGTK Wayland protocol crashes (Error 71) and eliminate white tearing lines during scrolling on NVIDIA.
5. **Tauri v2 Permissions:**
   - Any IPC command called from the remote origin must be registered in `src-tauri/permissions/*.toml` and enabled in `src-tauri/capabilities/default.json`.

## Common Commands
- Development: `npm run tauri dev`
- Native Arch Linux Build: `npm run build:arch` (produces `anivox-0.1.0-1-x86_64.pkg.tar.zst`)
- Install on Arch: `npm run install:arch` (or `sudo pacman -U anivox-0.1.0-1-x86_64.pkg.tar.zst`)
- Standard Universal Build: `npm run build` (produces AppImage, deb, rpm)
- Fast Rust Check: `cargo check --manifest-path src-tauri/Cargo.toml`
