use std::collections::HashMap;
use std::net::IpAddr;

use time::OffsetDateTime;
use tracing::info;

use super::counter::RateEstimator;
use crate::config::RuleConfig;
use crate::models::HitRecord;

/// State tracking for rules and active bans
#[derive(Default)]
pub struct StateMachine {
    /// rule_name -> ip -> rate estimator
    states: HashMap<String, HashMap<IpAddr, RateEstimator>>,
    /// Currently banned IPs: ip -> rule_name -> ban_end_time
    active_bans: HashMap<IpAddr, HashMap<String, OffsetDateTime>>,
}

impl StateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an attempt for a rule and IP, returns true if it exceeds threshold
    pub fn register_hit(&mut self, hit: &HitRecord, config: &RuleConfig) -> bool {
        let rule_entry = self.states.entry(config.name.clone()).or_default();
        let estimator =
            rule_entry.entry(hit.ip).or_insert_with(|| RateEstimator::new(config.window));
        estimator.push(hit.timestamp);
        info!(ip = %hit.ip, count = estimator.count(), threshold = config.max_attempts, "state update");
        if estimator.count() < config.max_attempts {
            return false;
        }

        let banned_until = OffsetDateTime::now_utc() + config.ban_duration;
        self.active_bans.entry(hit.ip).or_default().insert(config.name.clone(), banned_until);
        true
    }

    /// Return and remove expired bans as (IpAddr, rule_name)
    pub fn drain_expired_bans(&mut self) -> Vec<(IpAddr, String)> {
        let now = OffsetDateTime::now_utc();
        let mut expired = Vec::new();

        self.active_bans.retain(|ip, rules| {
            rules.retain(|rule_name, &mut end_time| {
                if end_time <= now {
                    expired.push((*ip, rule_name.clone()));
                    false
                } else {
                    true
                }
            });
            !rules.is_empty()
        });

        expired
    }
}
