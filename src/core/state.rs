use std::collections::HashMap;
use std::net::IpAddr;

use time::OffsetDateTime;
use tracing::info;

use super::counter::RateEstimator;
use crate::core::rule::Rule;
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
    pub fn register_hit(&mut self, hit: &HitRecord, rule: &Rule) -> Option<OffsetDateTime> {
        let rule_entry = self.states.entry(rule.name.clone()).or_default();
        let estimator = rule_entry.entry(hit.ip).or_insert_with(|| RateEstimator::new(rule.window));
        estimator.push(hit.timestamp);
        info!(ip = %hit.ip, count = estimator.count(), threshold = rule.max_attempts, "state update");
        if estimator.count() < rule.max_attempts {
            return None;
        }

        Some(OffsetDateTime::now_utc() + rule.ban_duration)
    }

    pub fn cleanup(&mut self, rules: &HashMap<String, Rule>, now: OffsetDateTime) {
        self.states.retain(|rule_name, ip_map| {
            // clean up if rule is missing (e.g. removed from config)
            let rule = match rules.get(rule_name) {
                Some(rule) => rule,
                None => {
                    info!(rule = %rule_name, "removing state for missing rule");
                    return false; // remove this rule entry entirely
                }
            };

            // remove expired IPs
            ip_map.retain(|ip, estimator| {
                let expired =
                    estimator.last_time().map(|last| now - last > rule.window).unwrap_or(true);

                if expired {
                    info!(ip = %ip, rule = %rule_name, "cleaning up state");
                }

                !expired
            });

            // if no IP left, remove the rule bucket
            !ip_map.is_empty()
        });
    }
}
