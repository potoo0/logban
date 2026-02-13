use std::collections::HashMap;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt;
use ipnet::IpNet;
use time::OffsetDateTime;
use tokio::sync::Mutex;
use tracing::{Instrument, error, info, info_span, warn};

use crate::config::RuleConfig;
use crate::core::action::Action;
use crate::core::matcher::RuleMatcher;
use crate::core::state::State;
use crate::core::store::Store;
use crate::models::{BanEntity, LogEntry};
use crate::source::LogSource;

const CLEANUP_BATCH_SIZE: i32 = 1024;

pub struct Engine {
    whitelist: Vec<IpNet>,
    actions: HashMap<String, Action>,
    rules: HashMap<String, RuleConfig>,
    store: Store,

    state: Arc<Mutex<State>>,
}

impl Engine {
    pub fn new(
        whitelist: Vec<IpNet>, actions: HashMap<String, Action>,
        rules: HashMap<String, RuleConfig>, store: Store,
    ) -> Result<Self> {
        let state = Arc::new(Mutex::new(State::new()));
        Ok(Self { whitelist, actions, rules, store, state })
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
                        let record = BanEntity::new(hit.ip, rule.name.clone(), end);
                        if let Err(e) = self.store.insert_active_ban(record).await {
                            error!("failed to record active ban: {}", e);
                        }
                    }
                }
            }
            .instrument(span)
            .await;
        }
    }

    async fn cleanup_expired_bans(&self, ts: i64) {
        loop {
            let expired = match self.store.fetch_expired_bans(ts, CLEANUP_BATCH_SIZE).await {
                Ok(rows) if !rows.is_empty() => rows,
                Err(e) => {
                    error!("failed to fetch expired bans: {}", e);
                    return;
                }
                _ => return,
            };
            let count = expired.len();
            let mut ids: Vec<i64> = Vec::with_capacity(count);
            for entity in expired {
                info!("unban: {} (expired)", entity.ip);
                // ignore all errors, just try the best effort to unban and clean up expired records
                if let Some(id) = entity.id {
                    ids.push(id);
                }

                let ip = match IpAddr::from_str(&entity.ip) {
                    Ok(ip) => ip,
                    Err(_) => {
                        warn!("invalid IP address in ban record: {}", entity.ip);
                        continue;
                    }
                };
                let rule = self.rules.get(&entity.rule).unwrap();
                let action = match self.actions.get(&rule.ban_action) {
                    Some(action) => action,
                    None => continue,
                };
                if let Err(e) = action.unban(ip, rule).await {
                    error!("unban failed: {}", e);
                }
            }

            if let Err(e) = self.store.mark_bans_inactive(ts, ids).await {
                warn!("failed to delete expired bans: {}", e);
            }

            // stop the loop if the batch is not full
            if count < CLEANUP_BATCH_SIZE as usize {
                return;
            }
        }
    }

    pub async fn run_cleanup_loop(&self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            interval.tick().await;
            let now = OffsetDateTime::now_utc().unix_timestamp();
            self.cleanup_expired_bans(now).await;
        }
    }
}
