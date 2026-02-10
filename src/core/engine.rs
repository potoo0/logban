use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt;
use ipnet::IpNet;
use tokio::sync::Mutex;
use tracing::{Instrument, error, info, info_span};

use crate::config::RuleConfig;
use crate::core::action::Action;
use crate::core::matcher::RuleMatcher;
use crate::core::state::StateMachine;
use crate::models::LogEntry;
use crate::source::LogSource;

pub struct Engine {
    whitelist: Vec<IpNet>,
    actions: HashMap<String, Action>,
    rules: HashMap<String, RuleConfig>,

    state: Arc<Mutex<StateMachine>>,
}

impl Engine {
    pub fn new(
        whitelist: Vec<IpNet>, actions: HashMap<String, Action>, rules: HashMap<String, RuleConfig>,
    ) -> Result<Self> {
        Ok(Self { whitelist, state: Arc::new(Mutex::new(StateMachine::new())), actions, rules })
    }

    pub async fn run_source(
        &self, mut source: Box<dyn LogSource>, matchers: &[RuleMatcher],
    ) -> Result<()> {
        info!("starting log source");
        let mut stream = source.stream()?;
        while let Some(entry) = stream.next().await {
            self.process_entry(entry, matchers).await;
        }
        Ok(())
    }

    async fn process_entry(&self, entry: LogEntry, matchers: &[RuleMatcher]) {
        for matcher in matchers {
            let rule = self.rules.get(&matcher.rule_name).unwrap();
            let action = match self.actions.get(&rule.ban_action) {
                Some(action) => action,
                None => continue,
            };
            // Set up tracing span for the rule
            let span = info_span!("rule", name = %rule.name);
            async {
                if let Some(hit) = matcher.match_entry(&entry) {
                    info!(ip = %hit.ip, "hit");
                    if self.whitelist.iter().any(|net| net.contains(&hit.ip)) {
                        info!("whitelisted: {}, skipping ban", hit.ip);
                        return;
                    }

                    let mut state = self.state.lock().await;
                    if let Some(end) = state.register_hit(&hit, rule) {
                        info!(ip = %hit.ip, ban_duration = ?rule.ban_duration, "ban");
                        if let Err(e) = action.ban(hit.ip, end, rule).await {
                            error!("ban failed: {}", e);
                        }
                    }
                }
            }
            .instrument(span)
            .await;
        }
    }

    pub async fn run_cleanup_loop(&self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            interval.tick().await;
            let mut state = self.state.lock().await;
            let expired = state.drain_expired_bans();

            for (ip, rule_name) in expired {
                info!("unban: {} (expired)", ip);

                let rule = self.rules.get(&rule_name).unwrap();
                let action = match self.actions.get(&rule.ban_action) {
                    Some(action) => action,
                    None => continue,
                };
                if let Err(e) = action.unban(ip, rule).await {
                    error!("unban failed: {}", e);
                }
            }
        }
    }
}
