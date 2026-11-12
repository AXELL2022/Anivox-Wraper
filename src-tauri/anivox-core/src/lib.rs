//! Platform-independent Anivox logic with no Tauri types.
//!
//! The Tauri shell (`src-tauri/src`) and any future native shell (WinUI 3,
//! GTK4, SwiftUI) wire these modules to windows, webviews and file locations.

pub mod connection;
pub mod discord_presence;
pub mod player_settings;
pub mod segments;
pub mod session_page;
pub mod shikimori;
pub mod shikimori_overlay;
pub mod site_hosts;
pub mod site_load;
pub mod tray_settings;
pub mod updater;

#[cfg(feature = "net")]
pub mod mpv;
#[cfg(feature = "net")]
pub mod mpv_session;
/// Site command dispatcher shared by the GTK and WinUI shells.
#[cfg(feature = "net")]
pub mod site_ipc;
