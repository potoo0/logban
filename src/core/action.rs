use std::collections::HashMap;
use std::net::IpAddr;

use anyhow::Result;
use time::OffsetDateTime;
use tokio::process::Command;
use tracing::{error, info};

use crate::config::{ActionConfig, RuleConfig};
use crate::utils::string::expand_template;

#[derive(Clone, Debug)]
pub struct Action {
    pub init: Option<String>,
    pub ban: String,
    pub unban: Option<String>,
    pub dry_run: bool,
}

impl Action {
    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    async fn run(&self, script: &str) -> Result<()> {
        if self.dry_run {
            info!(script = ?script, "dry-run mode, skipping execution");
            return Ok(());
        }

        let output = Command::new("sh").arg("-c").arg(script).output().await?;
        if output.status.success() {
            Ok(())
        } else {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!(script = ?script, stdout = ?stdout, stderr = ?stderr, "script execution failed");
            anyhow::bail!("execute script failed: {}", output.status)
        }
    }

    pub async fn init(&self) -> Result<()> {
        info!(dry_run = self.dry_run, "Initializing action");

        let init_script = match &self.init {
            Some(script) if !script.is_empty() => script,
            _ => {
                info!("init script is empty, skipping...");
                return Ok(());
            }
        };

        self.run(init_script).await
    }

    pub async fn ban(&self, ip: IpAddr, _end: OffsetDateTime, rule: &RuleConfig) -> Result<()> {
        let ip_str = ip.to_string();
        let timeout_sec = rule.ban_duration.as_secs().to_string();

        let mut vars = HashMap::with_capacity(2);
        vars.insert("ip", ip_str.as_str());
        vars.insert("timeout_sec", timeout_sec.as_str());
        let script = expand_template(self.ban.as_str(), &vars);
        self.run(script.as_ref()).await?;

        info!(ip = ?ip_str, ban_duration = ?rule.ban_duration, "ban finished");
        Ok(())
    }

    pub async fn unban(&self, ip: IpAddr, _rule: &RuleConfig) -> Result<()> {
        let unban_script = match &self.unban {
            Some(script) if !script.is_empty() => script,
            _ => {
                info!("unban script is empty, skipping unban for {}", ip);
                return Ok(());
            }
        };

        let ip_str = ip.to_string();
        let mut vars = HashMap::with_capacity(2);
        vars.insert("ip", ip_str.as_str());
        let script = expand_template(unban_script, &vars);
        self.run(script.as_ref()).await?;

        info!(ip = ?ip_str, "unban finished");
        Ok(())
    }
}

impl From<ActionConfig> for Action {
    fn from(cfg: ActionConfig) -> Self {
        Self { init: cfg.init, ban: cfg.ban, unban: cfg.unban, dry_run: false }
    }
}
