use std::net::IpAddr;
use std::str::FromStr;

use anyhow::{Context, Result};
use regex::Regex;
use tracing::{Level, trace};

use crate::config::RuleConfig;
use crate::models::{HitRecord, LogEntry};

/// `RuleMatcher` is responsible for matching log entries according to
/// a configured rule and extracting IP addresses.
pub struct RuleMatcher {
    pub rule_name: String,
    pattern: Vec<Regex>,
}

impl RuleMatcher {
    /// Attempts to match a log entry and extract an IP according to the rule.
    ///
    /// Matching flow:
    /// 1. Iterate over each regex in `pattern`.
    /// 2. Apply the regex to the current input line.
    /// 3. If a regex does not match, terminate immediately and return `None`.
    /// 4. If a match is found:
    ///     - If the named capture group `ip` exists, extract it as the candidate IP.
    ///     - Otherwise, take the last capture group of the current regex as candidate content for
    ///       the next pattern.
    /// 5. Use the candidate content as input for the next regex and repeat the process.
    /// 6. After all patterns are applied successfully:
    ///     - Attempt to parse the candidate IP as a valid IP address.
    ///     - If parsing succeeds, return `Some(HitRecord)` with the IP.
    ///     - If parsing fails or no candidate exists, return `None`.
    ///
    /// Returns:
    /// - `Some(HitRecord)` if all patterns match and a valid IP is extracted.
    /// - `None` if any pattern fails to match or if the candidate IP is invalid.
    pub fn match_entry(&self, entry: &LogEntry) -> Option<HitRecord> {
        trace!(entry.message = entry.message, "matching...");
        let mut candidate: &str = &entry.message;
        for (idx, re) in self.pattern.iter().enumerate() {
            // Pattern did not match, terminate early
            let caps = re.captures(candidate)?;

            // Trace matched groups
            if tracing::enabled!(Level::TRACE) {
                let caps_content: Vec<_> = caps
                    .iter()
                    .skip(1)
                    .map(|c| c.map(|m| format!("{}..{}/{:?}", m.start(), m.end(), m.as_str())))
                    .collect::<Vec<_>>();
                trace!(pattern.index = idx, groups = ?caps_content, "regex matched");
            }

            // Check for named "ip" group
            if let Some(ip_match) = caps.name("ip") {
                return IpAddr::from_str(ip_match.as_str())
                    .map(|ip| (entry.timestamp, ip).into())
                    .ok();
            }

            // Use last capture as candidate for next pattern
            candidate = match caps.iter().last().flatten() {
                Some(last_cap) => last_cap.as_str(),
                None => return None, // No capture group available
            };
        }
        None
    }
}

impl TryFrom<&RuleConfig> for RuleMatcher {
    type Error = anyhow::Error;

    fn try_from(config: &RuleConfig) -> Result<Self, Self::Error> {
        let pattern = config
            .pattern
            .iter()
            .map(|pat| {
                Regex::new(pat)
                    .with_context(|| format!("Failed to compile regex for rule '{}'", config.name))
            })
            .collect::<Result<Vec<Regex>>>()?;
        Ok(Self { rule_name: config.name.clone(), pattern })
    }
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::*;

    #[test]
    fn test_rule_matcher() {
        // let _ = tracing_subscriber::fmt()
        //     .with_writer(std::io::stderr)
        //     .with_max_level(Level::TRACE)
        //     .try_init();
        let rule_config = RuleConfig {
            name: "test_rule".to_string(),
            pattern: vec![
                r"^Failed password for (.*) from".into(),
                r"^(?P<ip>\d{1,3}(?:\.\d{1,3}){3})$".into(),
            ],
            window: Default::default(),
            max_attempts: Default::default(),
            ban_duration: Default::default(),
            ban_action: "nftables-allports".to_string(),
        };

        let matcher = RuleMatcher::try_from(&rule_config).unwrap();
        let cases = vec![
            ("password for 10.0.0.0 from", None),
            ("password for 1 from", None),
            ("Failed password for 10.0.0 from", None),
            ("Failed password for 10.0.0.0.0 from", None),
            ("Failed password for 10.0.0.0 from", Some(IpAddr::from_str("10.0.0.0").unwrap())),
            ("Failed password for 10.0.0.0 from 123", Some(IpAddr::from_str("10.0.0.0").unwrap())),
        ];
        for (idx, (msg, expected_ip)) in cases.into_iter().enumerate() {
            let entry = LogEntry { timestamp: OffsetDateTime::now_utc(), message: msg.to_string() };
            let span = tracing::info_span!("test_case", message_idx = idx);
            let _enter = span.enter();
            let hit = matcher.match_entry(&entry);
            assert_eq!(expected_ip, hit.as_ref().map(|h| h.ip));
        }
    }
}
