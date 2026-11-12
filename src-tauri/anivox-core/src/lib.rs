//! Platform-independent Anivox logic with no Tauri types.
//!
//! The Tauri shell (`src-tauri/src`) and any future native shell (WinUI 3,
//! GTK4, SwiftUI) wire these modules to windows, webviews and file locations.

pub mod discord_presence;
pub mod player_settings;
pub mod shikimori;
pub mod site_hosts;
pub mod tray_settings;

#[cfg(feature = "net")]
pub mod mpv;
#[cfg(feature = "net")]
pub mod mpv_session;
