use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("empty `{field}`{path_part}", path_part = fmt_path(path))]
    EmptyField { field: &'static str, path: Option<String> },

    #[error("duplicate `{field}`{path_part}, value: '{value}'", path_part = fmt_path(path))]
    DuplicateValue { field: &'static str, path: Option<String>, value: String },

    #[error("invalid `{field}`{path_part}, value: '{value}', reason: {reason}", path_part = fmt_path(path))]
    InvalidValue { field: &'static str, path: Option<String>, value: String, reason: String },
}

fn fmt_path(path: &Option<String>) -> String {
    path.as_deref().map(|p| format!(" at `{}`", p)).unwrap_or_default()
}
