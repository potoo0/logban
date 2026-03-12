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
use tracing::{Instrument, debug, info, info_span};

use crate::config::{Config, SourceConfig};
use crate::core::action::Action;
use crate::core::engine::Engine;
use crate::core::rule::Rule;
use crate::core::store::Store;
use crate::source::LogSource;
use crate::source::file::FileSource;
use crate::source::journal::JournalSource;
use crate::utils::string::truncate_tail;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    let args = cli::Args::parse();
    logging::init(args.log_level.clone())?;
    debug!("starting logban with command line args: {:?}", args);

    let cfg = Config::from_path(&args.config)?;
    debug!("loaded configuration: {:?}", cfg);
    let runtime = if let Some(worker_threads) = cfg.worker_threads {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(worker_threads)
            .enable_all()
            .build()?
    } else {
        tokio::runtime::Builder::new_multi_thread().enable_all().build()?
    };

    runtime.block_on(async {
        // Initialize store
        let store = if args.dry_run {
            info!("Using in-memory store for dry-run mode");
            Store::new_memory().await?
        } else {
            info!("Using file-based store at path: {}", cfg.db_file);
            Store::new_file(&cfg.db_file).await?
        };

        // Initialize actions
        let actions: HashMap<String, Action> = cfg
            .actions
            .into_iter()
            .map(|(name, action_cfg)| {
                (name.clone(), Action::from(action_cfg).with_dry_run(args.dry_run))
            })
            .collect();

        // Build all rules from sources and index them by name
        let rules = cfg
            .sources
            .iter()
            .flat_map(|source| match source {
                SourceConfig::Journal { rules, .. } | SourceConfig::File { rules, .. } => {
                    rules.iter()
                }
            })
            .map(|rc| {
                let rule = Rule::try_from((rc, &cfg.pattern_presets))?;
                Ok((rule.name.clone(), rule))
            })
            .collect::<Result<HashMap<_, _>, anyhow::Error>>()?;

        // Create engine
        let whitelist = cfg.whitelists.unwrap_or_default();
        let engine = Arc::new(Engine::new(whitelist, actions, rules, store)?);

        // Start cleanup task
        {
            let engine = Arc::clone(&engine);
            Builder::new().name("engine_cleanup").spawn(async move {
                engine.run_cleanup_loop().await;
            })?;
        }

        // Start source processing tasks
        let mut tasks = Vec::new();
        for source_config in cfg.sources {
            let (source, rule_configs) = match source_config {
                SourceConfig::Journal { unit, rules } => {
                    let source: Box<dyn LogSource> = Box::new(JournalSource::new(&unit)?);
                    (source, rules)
                }
                SourceConfig::File { path, rules } => {
                    let source: Box<dyn LogSource> = Box::new(FileSource::new(path)?);
                    (source, rules)
                }
            };

            let source_span = info_span!(
                "source",
                id = %truncate_tail(source.id(), 24),
                backend = %source.backend(),
            );

            let engine = engine.clone();
            let source_rules: Vec<String> = rule_configs.iter().map(|c| c.name.clone()).collect();
            tasks.push(
                Builder::new().name("run_source").spawn(
                    async move {
                        engine.run_source(source, source_rules).await.expect("Source run failed");
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
