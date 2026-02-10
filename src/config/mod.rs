pub mod errors;
mod tests;
pub mod validate;

use std::collections::HashMap;
use std::fs;
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use ipnet::IpNet;
use serde::Deserialize;

use self::validate::Validate;

pub static PROJECT_NAME: LazyLock<&'static str> = LazyLock::new(|| {
    let s = env!("CARGO_CRATE_NAME").replace('-', "_").to_ascii_uppercase();
    Box::leak(s.into_boxed_str())
});
type Presets = HashMap<String, String>;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Number of worker threads for processing log sources. Defaults to number of CPU cores.
    pub worker_threads: Option<usize>,
    /// IP networks to whitelist from banning
    pub whitelists: Option<Vec<IpNet>>,
    /// Action configurations, referenced by `source.rules[*].ban_action`
    pub actions: HashMap<String, ActionConfig>,
    /// Preset variables for use in `source.rules[*].pattern` via `${var}` syntax
    pub presets: Option<Presets>,
    /// Log sources configuration
    pub sources: Vec<SourceConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActionConfig {
    pub init: Option<String>,
    pub ban: String,
    pub unban: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum SourceConfig {
    Journal { unit: String, rules: Vec<RuleConfig> },
    File { path: String, rules: Vec<RuleConfig> },
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuleConfig {
    pub name: String,
    #[serde(with = "humantime_serde")]
    pub ban_duration: Duration,
    #[serde(with = "humantime_serde")]
    pub window: Duration,
    pub max_attempts: u32,
    pub ban_action: String,
    pub pattern: Vec<String>,
}

impl Config {
    pub fn from_path(path: &str) -> Result<Self> {
        let file = fs::File::open(path)
            .with_context(|| format!("failed to open config file `{}`", path))?;
        yaml_serde::from_reader::<_, Config>(file)
            .map_err(|err| anyhow!("invalid config: {}", err))?
            .validate()
    }

    #[allow(dead_code)]
    pub fn from_str(raw: &str) -> Result<Self> {
        yaml_serde::from_str::<Config>(raw)
            .map_err(|err| anyhow!("invalid config: {}", err))?
            .validate()
    }

    pub fn validate(self) -> Result<Self> {
        if self.sources.is_empty() {
            return Err(anyhow!("`source` cannot be empty"));
        }
        if let Some(presets) = &self.presets {
            presets.validate()?
        }
        for source in &self.sources {
            source.validate()?
        }
        self.validate_rule_ban_action()?;
        Ok(self)
    }

    fn validate_rule_ban_action(&self) -> Result<()> {
        for source in &self.sources {
            let rules = match source {
                SourceConfig::Journal { rules, .. } => rules,
                SourceConfig::File { rules, .. } => rules,
            };
            for rule in rules {
                if !self.actions.contains_key(&rule.ban_action) {
                    // TODO error type???
                    return Err(anyhow!(
                        "undefined ban_action `{}` in rule `{}`",
                        rule.ban_action,
                        rule.name
                    ));
                }
            }
        }
        Ok(())
    }
}
