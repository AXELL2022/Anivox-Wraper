# Project: Anivox Desktop (Tauri Web Wrapper)

## Project Overview
This project is a desktop application wrapper for the website `https://anivox.fun/`, built using the **Tauri v2** framework. It uses Rust for the backend and a minimal vanilla JS setup for the frontend (which primarily acts as a redirect/loader for the remote URL).

## Technologies
- **Framework:** Tauri v2
- **Backend:** Rust
- **Frontend:** Vanilla HTML/JS (redirects to remote site)
- **Package Manager:** npm

## Building and Running
> **Note:** You must have Rust installed on your system to build or run this project. Install it from [rust-lang.org](https://www.rust-lang.org/).

### Development
To run the application in development mode:
```bash
npm run tauri dev
```

### Production Build
To create a production bundle:
```bash
npm run tauri build
```

## Configuration
The main configuration file is located at `src-tauri/tauri.conf.json`.
- **Target URL:** `https://anivox.fun/`
- **Identifier:** `com.yasin.anivox`
- **Product Name:** `Anivox`

## Development Conventions
- **Surgical Changes:** Keep backend (Rust) logic in `src-tauri/src/main.rs`.
- **Remote Navigation:** The application is configured to load a remote URL directly. Ensure any necessary security configurations (like CSP) are updated in `tauri.conf.json` if additional features (like IPC) are needed for the remote site.
