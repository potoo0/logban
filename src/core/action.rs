use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, bail};
use time::OffsetDateTime;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::config::ActionConfig;
use crate::core::rule::Rule;
use crate::utils::string::expand_template;

#[derive(Debug)]
pub struct Action {
    init_cmd: Option<String>,
    ban_cmd: String,
    unban_cmd: Option<String>,
    dry_run: bool,

    init_lock: Mutex<()>,
    initialized: AtomicBool,
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
            bail!("execute script failed: {}", output.status)
        }
    }

    pub async fn init(&self) -> Result<()> {
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }

        // Fast path: if already initialized, return immediately without acquiring the lock
        info!(dry_run = self.dry_run, "Initializing action");

        // Check if init script is provided
        let init_script = match &self.init_cmd {
            Some(script) if !script.is_empty() => script,
            _ => {
                info!("init script is empty, skipping...");
                self.initialized.store(true, Ordering::Release);
                return Ok(());
            }
        };

        // Acquire the lock to ensure only one initializer runs the init script
        let _guard = self.init_lock.lock().await;
        // double check
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }

        match self.run(init_script).await {
            Ok(_) => {
                info!("action init finished");
                self.initialized.store(true, Ordering::Release);
                Ok(())
            }
            Err(e) => {
                bail!("action init failed: {}", e)
            }
        }
    }

    pub async fn ban(&self, ip: IpAddr, _end: OffsetDateTime, rule: &Rule) -> Result<()> {
        self.init().await?;
        let ip_str = ip.to_string();
        let timeout_sec = rule.ban_duration.as_secs().to_string();

        let mut vars = HashMap::with_capacity(2);
        vars.insert("rule_name", rule.name.as_str());
        vars.insert("ip", ip_str.as_str());
        vars.insert("timeout_sec", timeout_sec.as_str());
        let script = expand_template(self.ban_cmd.as_str(), &vars);
        self.run(script.as_ref()).await?;

        info!(ip = ?ip_str, ban_duration = ?rule.ban_duration, "ban finished");
        Ok(())
    }

    pub async fn unban(&self, ip: IpAddr, rule: &Rule) -> Result<()> {
        let unban_script = match &self.unban_cmd {
            Some(script) if !script.is_empty() => script,
            _ => {
                info!("unban script is empty, skipping unban for {}", ip);
                return Ok(());
            }
        };

        self.init().await?;
        let ip_str = ip.to_string();
        let mut vars = HashMap::with_capacity(2);
        vars.insert("rule_name", rule.name.as_str());
        vars.insert("ip", ip_str.as_str());
        let script = expand_template(unban_script, &vars);
        self.run(script.as_ref()).await?;

        info!(ip = ?ip_str, "unban finished");
        Ok(())
    }
}

impl From<ActionConfig> for Action {
    fn from(cfg: ActionConfig) -> Self {
        Self {
            init_cmd: cfg.init,
            ban_cmd: cfg.ban,
            unban_cmd: cfg.unban,
            dry_run: false,
            init_lock: Default::default(),
            initialized: Default::default(),
        }
    }
}
