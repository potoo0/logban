use anyhow::Result;
use tracing::Level;
use tracing_error::ErrorLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, fmt};

use crate::config::PROJECT_NAME;

pub fn init(log_level: Option<String>) -> Result<()> {
    // Resolve log filtering rules with the following priority:
    // 1. <PROJECT_NAME>_LOG_LEVEL (project-specific override)
    // 2. RUST_LOG (standard tracing environment variable)
    // 3. config.log_level (fallback, defaults to "info")
    let log_level = log_level.as_deref().unwrap_or(Level::INFO.as_str());
    let env_filter = EnvFilter::try_from_env(format!("{}_LOG_LEVEL", *PROJECT_NAME))
        .or_else(|_| EnvFilter::try_from_default_env())
        .or_else(|_| EnvFilter::try_new(log_level))?;

    let subscriber = fmt::layer().with_line_number(true).with_filter(env_filter);
    let registry = tracing_subscriber::registry().with(subscriber).with(ErrorLayer::default());

    #[cfg(debug_assertions)]
    let registry = registry.with(console_subscriber::spawn());

    registry.try_init()?;

    Ok(())
}
