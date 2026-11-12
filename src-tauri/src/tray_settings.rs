use std::sync::atomic::AtomicBool;

pub(crate) use anivox_core::tray_settings::{load, save};

pub(crate) struct TrayState(pub AtomicBool);

impl Default for TrayState {
    fn default() -> Self {
        Self(AtomicBool::new(true))
    }
}
