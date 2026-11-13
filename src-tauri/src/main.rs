//! Tauri now builds only the minimal macOS application (`macos.rs`). Windows and
//! Linux use the native shells `windows-winui/` and `linux-gtk/`.

#[cfg(target_os = "macos")]
#[path = "discord_presence.rs"]
mod discord;
#[cfg(all(target_os = "macos", test))]
mod discord_tests;
#[cfg(target_os = "macos")]
mod macos;
// tauri-build needs the dependency on every platform; only macOS uses it.
#[cfg(not(target_os = "macos"))]
use tauri as _;

fn main() {
    #[cfg(target_os = "macos")]
    macos::run();

    // Keeps `cargo test --workspace` (the core) working on Windows and Linux.
    #[cfg(not(target_os = "macos"))]
    eprintln!(
        "Tauri-сборка Anivox поддерживает только macOS: используйте windows-winui или linux-gtk."
    );
}
