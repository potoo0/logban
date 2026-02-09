use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt;
use ipnet::IpNet;
use tokio::sync::Mutex;
use tracing::{Instrument, error, info, info_span};

use crate::action::Action;
use crate::core::rule::RuleMatcher;
use crate::core::state::StateMachine;
use crate::models::LogEntry;
use crate::source::LogSource;

pub struct Engine {
    whitelist: Vec<IpNet>,
    state: Arc<Mutex<StateMachine>>,
    action: Arc<dyn Action>,
}

impl Engine {
    pub fn new(whitelist: Vec<IpNet>, action: Arc<dyn Action>) -> Result<Self> {
        Ok(Self { whitelist, state: Arc::new(Mutex::new(StateMachine::new())), action })
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
            // Set up tracing span for the rule
            let span = info_span!("rule", name = %matcher.config().name);
            async {
                if let Some(hit) = matcher.match_entry(&entry) {
                    info!(ip = %hit.ip, "hit");
                    if self.whitelist.iter().any(|net| net.contains(&hit.ip)) {
                        info!("whitelisted: {}, skipping ban", hit.ip);
                        return;
                    }

                    let mut state = self.state.lock().await;
                    if state.register_hit(&hit, matcher.config()) {
                        info!(ip = %hit.ip, ban_duration = ?matcher.config().ban_duration, "ban");
                        if let Err(e) = self.action.ban(hit.ip, &matcher.config().name).await {
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
                if let Err(e) = self.action.unban(ip, &rule_name).await {
                    error!("unban failed: {}", e);
                }
            }
        }
    }
}
