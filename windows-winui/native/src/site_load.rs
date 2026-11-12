//! Load attempts of the site document (same rules as linux-gtk `site_recovery.rs`).
//!
//! A navigation starts a new attempt (generation); `site-lifecycle.js` reports
//! when the site's app has mounted. Only the current attempt can finish.

use anivox_core::site_hosts;
use std::sync::{Arc, Mutex};
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Loading,
    Ready,
    Timeout,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadInfo {
    pub phase: Phase,
    pub detail: Option<String>,
    pub generation: u64,
}

impl Default for LoadInfo {
    fn default() -> Self {
        Self {
            phase: Phase::Loading,
            detail: None,
            generation: 0,
        }
    }
}

impl LoadInfo {
    pub fn begin(&mut self) -> u64 {
        self.generation += 1;
        self.phase = Phase::Loading;
        self.detail = None;
        self.generation
    }

    /// Only the current attempt can finish; a late report of an older one is ignored.
    pub fn finish(&mut self, generation: u64) -> bool {
        if self.generation != generation {
            return false;
        }
        self.phase = Phase::Ready;
        self.detail = None;
        true
    }

    pub fn timeout(&mut self, generation: u64) -> bool {
        if self.generation != generation || self.phase != Phase::Loading {
            return false;
        }
        self.phase = Phase::Timeout;
        true
    }

    pub fn fail(&mut self, detail: String) {
        self.phase = Phase::Error;
        self.detail = Some(detail);
    }
}

pub type SharedLoad = Arc<Mutex<LoadInfo>>;

/// Validates a `site_document_state` report against the page the shell shows.
/// Returns whether it may finish the given attempt.
pub fn document_report_matches(page: &str, href: &str) -> Result<bool, String> {
    let page = Url::parse(page).map_err(|err| err.to_string())?;
    let url = Url::parse(href).map_err(|err| err.to_string())?;
    if !site_hosts::is_site_origin(&page) || url.origin() != page.origin() {
        return Err("Недопустимый адрес сайта".into());
    }
    // A late document-ready from a previous navigation must not hide recovery.
    Ok(url.path() == page.path() && url.query() == page.query())
}
