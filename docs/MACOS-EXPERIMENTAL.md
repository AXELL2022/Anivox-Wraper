# Experimental minimal macOS build

Branch: `experimental/macos-minimal`. This is a build experiment, not a tested macOS release.

## Included

- A single WKWebView showing `https://anivox.fun/`.
- Native application/navigation menu.
- Portable dark player/control styling, 250 ms double-click fullscreen with playback-state preservation, and a small floating Back/Forward/Reload/Player Fullscreen control strip.
- Discord Rich Presence using the shared Rust backend: anime/episode/season, artwork, public links, pause/buffering and speed-aware remaining timer. Heartbeat retries recover when Discord starts or restarts. Discord desktop must be installed/running.
- Native, explicitly confirmed Tauri updater flow using the existing public key and HTTPS endpoint.
- Separate Apple Silicon and Intel app/DMG artifacts from GitHub Actions.

## Deliberately excluded

MPV, VPN/WireGuard, Shikimori overlay, custom Windows/Linux toolbar, tray integration, media mirroring and platform-specific video hacks.

The macOS binary selects `src/macos.rs`; the legacy integration library is excluded on macOS. Discord helpers are shared with Windows/Linux. The automatically merged `tauri.macos.conf.json` selects only a narrow Discord capability for the `macos-main` webview at `https://anivox.fun/*`; the command additionally validates its caller. Other origins receive no native permissions. `inject-macos.js` adds portable UI and observes real player state without intercepting media requests or loading MPV scripts.

## Build

On a Mac with the Tauri prerequisites:

```sh
bun install --frozen-lockfile
bun run tauri build --target aarch64-apple-darwin
# or x86_64-apple-darwin with that Rust target installed
```

The `Experimental macOS Minimal` workflow builds both targets and uploads artifacts. It has read-only repository permissions, creates no tags/releases and does not update the stable updater manifest. Updater archives use the repository's existing Tauri signing secrets; Apple signing is ad-hoc, with no notarization. Updater signatures are not Apple Developer ID signatures.

## Updater limitations

The updater plugin is included, but the stable `latest.json` may have no Darwin entry. Until compatible signed macOS artifacts and their `darwin-aarch64`/`darwin-x86_64` manifest entries are published, checking for updates may report an unavailable target. This experiment deliberately does not modify the stable feed or publish releases. A full install/update cycle remains untested.

## Validation

CI compile/tests and successful packaging are not proof of GUI functionality. On a real Mac check navigation/login, playback/audio/subtitles, fullscreen/Escape, native menu shortcuts, window close/reopen behaviour and updater errors/confirmation. Test DMG installation and Gatekeeper handling separately; unsigned/ad-hoc builds may be blocked and must not be represented as notarized releases.
