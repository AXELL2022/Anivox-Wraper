// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
#[path = "discord_presence.rs"]
mod discord;
#[cfg(all(target_os = "macos", test))]
mod discord_tests;
#[cfg(target_os = "macos")]
mod macos;
// Shared with the library; only the origin helpers are used on macOS.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
mod site_hosts;

fn main() {
    #[cfg(target_os = "macos")]
    macos::run();

    #[cfg(not(target_os = "macos"))]
    anivox_lib::run();
}
