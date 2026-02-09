mod action;
mod cli;
mod config;
mod core;
mod logging;
mod models;
mod source;
mod utils;

use std::sync::Arc;

use clap::Parser;
use tokio::task::Builder;
use tracing::{Instrument, info, info_span};

use crate::action::nftables::NftAction;
use crate::config::{Config, SourceConfig};
use crate::core::engine::Engine;
use crate::core::rule::RuleMatcher;
use crate::source::LogSource;
use crate::source::file::FileSource;
use crate::source::journal::JournalSource;
use crate::utils::string::truncate_tail;

const NFT_TABLE: &str = "logban";
const NFT_SET: &str = "banned_ips";

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    let args = cli::Args::parse();
    logging::init(args.log_level)?;
    info!("Loading config from: {}", args.config);

    let cfg = Config::from_path(&args.config)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(cfg.worker_threads.unwrap_or(2))
        .enable_all()
        .build()?;

    runtime.block_on(async {
        // TODO init actions based on config
        let nft = Arc::new(NftAction::new(NFT_TABLE, NFT_SET, args.dry_run));
        nft.init().await?;

        let whitelist = cfg.whitelists.unwrap_or_default();
        let engine = Arc::new(Engine::new(whitelist, nft)?);
        let engine_cleanup = Arc::clone(&engine);
        Builder::new().name("engine_cleanup").spawn(async move {
            engine_cleanup.run_cleanup_loop().await;
        })?;

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
            let matchers = rules
                .into_iter()
                .map(RuleMatcher::try_from)
                .collect::<Result<Vec<RuleMatcher>, _>>()?;

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
