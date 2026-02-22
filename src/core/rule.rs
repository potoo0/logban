use std::net::IpAddr;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Error, Result};
use regex::Regex;
use tracing::{Level, trace};

use crate::config::{Presets, RuleConfig};
use crate::models::{HitRecord, LogEntry};
use crate::utils::string::expand_template;

#[derive(Debug, Clone)]
pub struct Rule {
    pub name: String,
    pub ban_duration: Duration,
    pub window: Duration,
    pub max_attempts: u32,
    pub ban_action: String,
    pub pattern: Vec<Regex>,
}

impl Rule {
    /// Attempts to match a log entry and extract an IP according to the rule.
    ///
    /// Matching flow:
    /// 1. Start with the full log line as candidate content.
    /// 2. Apply each pattern in order:
    ///    - If it doesn't match, stop and skip the line.
    ///    - If a named capture group `ip` exists:
    ///        - Try to parse it as an IP.
    ///        - If parsing succeeds, return HitRecord immediately.
    ///        - If parsing fails, continue with the next pattern.
    ///    - Otherwise, take the last capture group as candidate for the next pattern.
    /// 3. If no IP is found after all patterns, skip the line.
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

impl TryFrom<(&RuleConfig, &Option<Presets>)> for Rule {
    type Error = Error;

    fn try_from(value: (&RuleConfig, &Option<Presets>)) -> Result<Self, Self::Error> {
        let pattern = value
            .0
            .pattern
            .iter()
            .map(|pat| build_pattern(pat, value.1))
            .collect::<Result<Vec<Regex>>>()?;
        Ok(Self {
            name: value.0.name.clone(),
            ban_duration: value.0.ban_duration,
            window: value.0.window,
            max_attempts: value.0.max_attempts,
            ban_action: value.0.ban_action.clone(),
            pattern,
        })
    }
}

fn build_pattern(pat: &str, presets: &Option<Presets>) -> Result<Regex> {
    let expanded = match presets {
        Some(vars) => expand_template(pat, vars),
        None => pat.into(),
    };
    Regex::new(&expanded).with_context(|| format!("Failed to compile regex pattern '{}'", expanded))
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
                r"^Failed password for (?:.*) from (.*) port".into(),
                r"^(?P<ip>\d{1,3}(?:\.\d{1,3}){3})$".into(),
            ],
            window: Default::default(),
            max_attempts: Default::default(),
            ban_duration: Default::default(),
            ban_action: "nftables-allports".to_string(),
        };
        let presets = None;

        let rule = Rule::try_from((&rule_config, &presets)).unwrap();
        let cases = vec![
            ("password for root from 10.0.0.0 port", None),
            ("password for root from 1 port", None),
            ("Failed password for root from 10.0.0 port", None),
            ("Failed password for root from 10.0.0.0.0 port", None),
            ("Failed password for root from 10.0.0.0", None),
            (
                "Failed password for root from 10.0.0.0 port",
                Some(IpAddr::from_str("10.0.0.0").unwrap()),
            ),
            (
                "Failed password for root from 10.0.0.0 port 42481",
                Some(IpAddr::from_str("10.0.0.0").unwrap()),
            ),
        ];
        for (idx, (msg, expected_ip)) in cases.into_iter().enumerate() {
            let entry = LogEntry { timestamp: OffsetDateTime::now_utc(), message: msg.into() };
            let span = tracing::info_span!("test_case", message_idx = idx);
            let _enter = span.enter();
            let hit = rule.match_entry(&entry);
            assert_eq!(expected_ip, hit.as_ref().map(|h| h.ip));
        }
    }
}
