//! Load attempts of the site document behind the native recovery screen.
//!
//! A navigation starts a new attempt (generation); `site-lifecycle.js` reports
//! when the site's app has mounted. Only the current attempt can finish.

use crate::site_hosts;
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

/// A finished real HTTP(S) document on a foreign host (e.g. a login page) also
/// ends the wait: `site-lifecycle.js` runs only on the site itself.
pub fn is_external_document(uri: &str) -> bool {
    Url::parse(uri).is_ok_and(|url| {
        matches!(url.scheme(), "https" | "http")
            && url
                .host_str()
                .is_some_and(|host| !site_hosts::is_site_host(host))
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_reload_completion_and_watchdog_cannot_finish_current_attempt() {
        let mut state = LoadInfo::default();
        state.begin();
        let current = state.begin();
        assert!(!state.finish(current - 1));
        assert!(!state.timeout(current - 1));
        assert_eq!(state.phase, Phase::Loading);
        assert!(state.finish(current));
        assert!(!state.timeout(current));
        assert_eq!(state.phase, Phase::Ready);
    }

    #[test]
    fn slow_document_can_finish_after_timeout_without_a_reload() {
        let mut state = LoadInfo::default();
        let current = state.begin();
        assert!(state.timeout(current));
        assert_eq!(state.phase, Phase::Timeout);
        assert!(state.finish(current));
        assert_eq!(state.phase, Phase::Ready);
    }

    #[test]
    fn blank_engine_and_site_documents_cannot_dismiss_loading() {
        for value in [
            "about:blank",
            "data:text/html,",
            "https://anivox.fun/",
            "https://anivox.me/",
        ] {
            assert!(!is_external_document(value), "{value}");
        }
        assert!(is_external_document("https://accounts.google.com/"));
    }

    #[test]
    fn document_reports_must_match_the_shown_page() {
        let page = "https://anivox.fun/anime/1-x?episode=2";
        assert!(document_report_matches(page, page).unwrap());
        assert!(!document_report_matches(page, "https://anivox.fun/").unwrap());
        assert!(document_report_matches(page, "https://anivox.me/anime/1-x").is_err());
        assert!(document_report_matches("https://evil.test/", "https://evil.test/").is_err());
    }
}
