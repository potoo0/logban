use std::collections::HashSet;

use regex::Regex;

use super::errors::ConfigError;
use super::{Presets, RuleConfig, SourceConfig};

pub trait Validate {
    fn validate(&self) -> Result<(), ConfigError>;
}

/// Check that preset keys and values are valid.
///
/// Rules:
/// - Keys must not be empty
/// - Keys must be valid identifiers (letters, digits, underscore; cannot start with a digit)
/// - Values must not be empty
impl Validate for Presets {
    fn validate(&self) -> Result<(), ConfigError> {
        let key_re = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap();
        for (k, v) in self {
            if k.trim().is_empty() {
                return Err(ConfigError::EmptyPresetKey(k.clone()));
            }
            if !key_re.is_match(k) {
                return Err(ConfigError::InvalidPresetKey(k.clone()));
            }
            if v.trim().is_empty() {
                return Err(ConfigError::EmptyPresetValue(k.clone()));
            }
        }
        Ok(())
    }
}

/// Check that source configuration is valid.
///
/// Rules:
/// - For Journal sources, the unit must not be empty
/// - For File sources, the path must not be empty
/// - The rules must be valid (see `Validate for Vec<RuleConfig>`)
impl Validate for SourceConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        match self {
            SourceConfig::Journal { unit, rules } => {
                if unit.trim().is_empty() {
                    return Err(ConfigError::EmptyUnit);
                }
                rules.validate()
            }

            SourceConfig::File { path, rules } => {
                if path.trim().is_empty() {
                    return Err(ConfigError::EmptyPath);
                }
                rules.validate()
            }
        }
    }
}

/// Check that rule configurations are valid.
///
/// Rules:
/// - The list of rules must not be empty
/// - Each rule must have a non-empty unique name and pattern
impl Validate for Vec<RuleConfig> {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.is_empty() {
            return Err(ConfigError::EmptyRules);
        }
        let mut names = HashSet::with_capacity(self.len());
        for rule in self {
            if rule.name.is_empty() {
                return Err(ConfigError::EmptyRuleName);
            }
            if names.contains(&rule.name) {
                return Err(ConfigError::DuplicateRuleName(rule.name.clone()));
            }
            names.insert(rule.name.clone());
            if rule.pattern.is_empty() {
                return Err(ConfigError::EmptyPattern);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use regex::Regex;

    #[test]
    fn test_preset_key_pattern() {
        let key_re = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap();

        let cases = vec![
            ("valid_key", true),
            ("_validKey2", true),
            ("anotherKey1", true),
            ("1invalidKey", false),
            ("invalid-key", false),
        ];
        for (key, is_valid) in cases {
            let actual = key_re.is_match(key);
            assert_eq!(
                actual, is_valid,
                "Key `{}` validation failed: expected {}, got {}",
                key, is_valid, actual
            );
        }
    }
}
