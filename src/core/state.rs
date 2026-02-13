use std::collections::HashMap;
use std::net::IpAddr;

use time::OffsetDateTime;
use tracing::info;

use super::counter::RateEstimator;
use crate::config::RuleConfig;
use crate::models::HitRecord;

/// State tracking for rules and active bans
#[derive(Default)]
pub struct State {
    /// rule_name -> ip -> rate estimator
    states: HashMap<String, HashMap<IpAddr, RateEstimator>>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an attempt for a rule and IP, returns banned_until if it exceeds threshold
    pub fn register_hit(&mut self, hit: &HitRecord, config: &RuleConfig) -> Option<OffsetDateTime> {
        let rule_entry = self.states.entry(config.name.clone()).or_default();
        let estimator =
            rule_entry.entry(hit.ip).or_insert_with(|| RateEstimator::new(config.window));
        estimator.push(hit.timestamp);
        info!(ip = %hit.ip, count = estimator.count(), threshold = config.max_attempts, "state update");
        if estimator.count() < config.max_attempts {
            return None;
        }

        Some(OffsetDateTime::now_utc() + config.ban_duration)
    }
}
