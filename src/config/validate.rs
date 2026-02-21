use std::collections::HashSet;

use regex::Regex;

use super::errors::ConfigError;
use super::{Presets, RuleConfig, SourceConfig};

pub trait Validate {
    fn validate(&self, path: &str) -> Result<(), ConfigError>;
}

/// Check that preset keys and values are valid.
///
/// Rules:
/// - Keys must not be empty
/// - Keys must be valid identifiers (letters, digits, underscore; cannot start with a digit)
/// - Values must not be empty
impl Validate for Presets {
    fn validate(&self, path: &str) -> Result<(), ConfigError> {
        let key_re = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap();
        for (k, v) in self {
            let key_path = format!("{}.{}", path, k);
            if k.trim().is_empty() {
                return Err(ConfigError::EmptyField {
                    field: "preset key",
                    path: Some(path.into()),
                });
            }
            if !key_re.is_match(k) {
                return Err(ConfigError::InvalidValue {
                    field: "preset key",
                    value: k.clone(),
                    reason: "must be a valid identifier".to_string(),
                    path: Some(key_path),
                });
            }
            if v.trim().is_empty() {
                return Err(ConfigError::EmptyField {
                    field: "preset value",
                    path: Some(key_path),
                });
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
    fn validate(&self, path: &str) -> Result<(), ConfigError> {
        match self {
            SourceConfig::Journal { unit, rules } => {
                if unit.trim().is_empty() {
                    return Err(ConfigError::EmptyField {
                        field: "unit",
                        path: Some(format!("{}.unit", path)),
                    });
                }
                rules.validate(&format!("{}.rules", path))
            }

            SourceConfig::File { path: file_path, rules } => {
                if file_path.trim().is_empty() {
                    return Err(ConfigError::EmptyField {
                        field: "path",
                        path: Some(format!("{}.path", path)),
                    });
                }
                rules.validate(&format!("{}.rules", path))
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
    fn validate(&self, path: &str) -> Result<(), ConfigError> {
        if self.is_empty() {
            return Err(ConfigError::EmptyField { field: "rules", path: Some(path.into()) });
        }
        let mut names = HashSet::with_capacity(self.len());
        for (i, rule) in self.iter().enumerate() {
            let rule_path = format!("{}[{}]", path, i);
            if rule.name.is_empty() {
                return Err(ConfigError::EmptyField {
                    field: "name",
                    path: Some(format!("{}.name", rule_path)),
                });
            }
            if names.contains(&rule.name) {
                return Err(ConfigError::DuplicateValue {
                    field: "rule name",
                    value: rule.name.clone(),
                    path: Some(format!("{}.name", rule_path)),
                });
            }
            names.insert(rule.name.clone());
            if rule.pattern.is_empty() {
                return Err(ConfigError::EmptyField {
                    field: "pattern",
                    path: Some(format!("{}.pattern", rule_path)),
                });
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
