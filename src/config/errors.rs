use thiserror::Error;

// TODO improve error handling, e.g., add context to rule errors
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Empty unit for source ? at ?")]
    EmptyUnit,
    #[error("Empty path for source ? at ?")]
    EmptyPath,
    #[error("Empty rule.name for source ? at ?")]
    EmptyRuleName,
    #[error("Duplicate rule.name `{0}` for source ? at ?")]
    DuplicateRuleName(String),
    #[error("Empty pattern for rule ? at ?")]
    EmptyPattern,
    #[error("Empty rules at ?")]
    EmptyRules,
    // #[error("Invalid max_attempts for rule ? at ?")]
    // InvalidMaxAttempts,
    #[error("Empty preset key `{0}`")]
    EmptyPresetKey(String),
    #[error("Empty preset value for key `{0}`")]
    EmptyPresetValue(String),
    #[error("Invalid preset key `{0}`, must be a valid identifier")]
    InvalidPresetKey(String),
}
