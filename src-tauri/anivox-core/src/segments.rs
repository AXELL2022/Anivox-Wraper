//! Video segment summary reported by `segment-monitor.js` for the connection check.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SegmentStats {
    pub host: String,
    pub active: bool,
    pub inflight: u32,
    pub stalled_ms: u64,
    pub has_speed: bool,
    pub mbps: f64,
    pub slow: u32,
    pub failed: u32,
    pub samples: u32,
    pub max_ms: u64,
    pub updated_at: f64,
}

impl SegmentStats {
    /// Values come from a web page: bound the host and drop impossible speeds.
    pub fn sanitized(mut self) -> Self {
        self.host = self.host.chars().take(120).collect();
        if !self.mbps.is_finite() || self.mbps < 0.0 {
            self.mbps = 0.0;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_values_are_bounded() {
        let stats = SegmentStats {
            host: "x".repeat(500),
            mbps: f64::NAN,
            ..SegmentStats::default()
        }
        .sanitized();
        assert_eq!(stats.host.len(), 120);
        assert_eq!(stats.mbps, 0.0);
    }
}
