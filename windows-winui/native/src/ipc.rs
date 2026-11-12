//! Site command dispatcher: the shared [`anivox_core::site_ipc`] with UI events
//! delivered to the WinUI shell as JSON through the callback.

pub use anivox_core::site_ipc::{
    EventSink as UiSink, SiteEvent as UiEvent, SiteHost, SiteIpc as Ipc,
};
use std::path::PathBuf;

pub const USER_AGENT: &str = concat!("Anivox/", env!("CARGO_PKG_VERSION"));

pub fn host(ui: UiSink, config_dir: PathBuf, data_dir: PathBuf) -> SiteHost {
    SiteHost::new(ui, USER_AGENT, config_dir, data_dir)
}
