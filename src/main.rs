mod cli;
mod config;
mod core;
mod logging;
mod models;
mod source;
mod utils;

use std::collections::HashMap;
use std::sync::Arc;

use clap::Parser;
use tokio::task::Builder;
use tracing::{Instrument, info, info_span};

use crate::config::{Config, SourceConfig};
use crate::core::action::Action;
use crate::core::engine::Engine;
use crate::core::matcher::RuleMatcher;
use crate::source::LogSource;
use crate::source::file::FileSource;
use crate::source::journal::JournalSource;
use crate::utils::string::truncate_tail;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    let args = cli::Args::parse();
    logging::init(args.log_level)?;
    info!("Loading config from: {}", args.config);

    let cfg = Config::from_path(&args.config)?;
    let runtime = if let Some(worker_threads) = cfg.worker_threads {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(worker_threads)
            .enable_all()
            .build()?
    } else {
        tokio::runtime::Builder::new_multi_thread().enable_all().build()?
    };

    runtime.block_on(async {
        // init actions
        let actions: HashMap<String, Action> = cfg
            .actions
            .into_iter()
            .map(|(name, action_cfg)| {
                (name.clone(), Action::from(action_cfg).with_dry_run(args.dry_run))
            })
            .collect();
        for (name, action) in &actions {
            let span = info_span!("action", name = name);
            action.init().instrument(span).await?;
        }

        // build engine
        let whitelist = cfg.whitelists.unwrap_or_default();
        let rules = cfg
            .sources
            .iter()
            .flat_map(|source| match source {
                SourceConfig::Journal { rules, .. } | SourceConfig::File { rules, .. } => {
                    rules.clone()
                }
            })
            .map(|rule_cfg| (rule_cfg.name.clone(), rule_cfg))
            .collect();
        let engine = Arc::new(Engine::new(whitelist, actions, rules)?);
        let engine_cleanup = Arc::clone(&engine);
        Builder::new().name("engine_cleanup").spawn(async move {
            engine_cleanup.run_cleanup_loop().await;
        })?;

        // run sources
        let mut tasks = Vec::new();
        for source in cfg.sources {
            let (source, rules) = match source {
                SourceConfig::Journal { unit, rules } => {
                    let source: Box<dyn LogSource> = Box::new(JournalSource::new(unit)?);
                    (source, rules)
                }
                SourceConfig::File { path, rules } => {
                    let source: Box<dyn LogSource> = Box::new(FileSource::new(path)?);
                    (source, rules)
                }
            };
            let engine_source = engine.clone();
            let matchers =
                rules.iter().map(RuleMatcher::try_from).collect::<Result<Vec<RuleMatcher>, _>>()?;

            let source_span = info_span!(
                "source",
                id = %truncate_tail(source.id(), 24),
                backend = %source.backend(),
            );
            tasks.push(
                Builder::new().name("run_source").spawn(
                    async move {
                        // TODO : handle panic
                        engine_source
                            .run_source(source, &matchers)
                            .await
                            .expect("Source run failed");
                    }
                    .instrument(source_span),
                )?,
            );
        }

        info!("Logban is running. Press Ctrl+C to stop.");
        futures::future::join_all(tasks).await;
        Ok(())
    })
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This example only runs on Linux.");
}
