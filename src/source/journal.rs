use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, RwLock};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use futures::StreamExt;
use futures::stream::BoxStream;
use systemd::journal;
use systemd::journal::{Journal, JournalWaitResult};
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, info, warn};

use super::LogSource;
use crate::models::LogEntry;

//
// =====================
// SystemdSource
// =====================
//
pub struct JournalSource {
    unit: String,
    rx: Option<mpsc::Receiver<LogEntry>>,
}

impl JournalSource {
    pub fn new(unit: &str) -> Result<Self> {
        Ok(Self { unit: unit.to_string(), rx: None })
    }
}

impl LogSource for JournalSource {
    fn id(&self) -> &str {
        &self.unit
    }

    fn backend(&self) -> &str {
        "systemd"
    }

    fn stream(&mut self) -> Result<BoxStream<'_, LogEntry>> {
        if self.rx.is_none() {
            let hub = JournalHub::global();
            hub.start();
            self.rx = Some(hub.subscribe(&self.unit));
        }

        Ok(ReceiverStream::new(self.rx.take().unwrap()).boxed())
    }
}

//
// =====================
// JournalHub
// =====================
//
#[derive(Default)]
struct JournalHub {
    // Ensure reader starts only once
    started: AtomicBool,
    // Map unit -> sender
    subscribers: RwLock<HashMap<String, mpsc::Sender<LogEntry>>>,
}

// Global singleton
static HUB: LazyLock<Arc<JournalHub>> = LazyLock::new(|| Arc::new(Default::default()));

impl JournalHub {
    pub fn global() -> Arc<Self> {
        HUB.clone()
    }

    // Lazily start journal reader
    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }

        let hub = self.clone();
        // Run blocking journal loop
        thread::Builder::new()
            .name("journal-hub".into())
            .spawn(move || {
                if let Err(e) = hub.run_loop() {
                    error!("journal loop error: {:?}", e);
                }
            })
            .expect("failed to start journal thread");
    }

    // Register a unit subscriber
    pub fn subscribe(&self, unit: &str) -> mpsc::Receiver<LogEntry> {
        let (tx, rx) = mpsc::channel(512);

        self.subscribers.write().unwrap().insert(unit.to_string(), tx);

        rx
    }
}

impl JournalHub {
    /// Add matches for all subscribed units to the journal
    fn match_units(&self, journal: &mut Journal) -> Result<()> {
        let units = {
            let guard = self.subscribers.read().unwrap();
            guard.keys().cloned().collect::<Vec<_>>()
        };
        info!("starting journal loop with units: {:?}", units);
        journal.match_flush()?;
        for (idx, unit) in units.into_iter().enumerate() {
            if idx > 0 {
                journal.match_or()?;
            }
            journal.match_add("_SYSTEMD_UNIT", unit.as_str())?;
        }
        Ok(())
    }

    fn run_loop(self: Arc<Self>) -> Result<()> {
        let mut journal: Journal = journal::OpenOptions::default().open()?;

        // add matches
        self.match_units(&mut journal)?;
        // seek to the end of journal to only get new entries
        seek_tail(&mut journal)?;

        loop {
            match journal.wait(None)? {
                JournalWaitResult::Append => {
                    while journal.next()? > 0 {
                        if let Err(e) = self.process_entry(&mut journal) {
                            warn!("failed to process journal entry: {:?}", e);
                        }
                    }
                }
                JournalWaitResult::Invalidate => {
                    // Journal rotated, reposition to tail
                    seek_tail(&mut journal)?;
                }
                _ => {}
            }
        }
    }

    fn process_entry(&self, journal: &mut Journal) -> Result<()> {
        let ts = {
            let ts = journal.timestamp_usec()?;
            OffsetDateTime::from_unix_timestamp_nanos((ts as i128) * 1_000)?
        };

        let message = match journal.get_data("MESSAGE")? {
            Some(v) => {
                let bytes = v.data();
                let value_bytes = &bytes["MESSAGE=".len()..];
                str::from_utf8(value_bytes)?.into()
            }
            None => return Ok(()),
        };

        if let Some(v_unit) = journal.get_data("_SYSTEMD_UNIT")? {
            let bytes = v_unit.data();
            let unit = str::from_utf8(&bytes["_SYSTEMD_UNIT=".len()..])?;

            let entry = LogEntry { timestamp: ts, message };
            self.dispatch(unit, entry);
        }

        Ok(())
    }

    fn dispatch(&self, unit: &str, entry: LogEntry) {
        let sender = {
            let map = self.subscribers.read().unwrap();
            map.get(unit).cloned()
        };

        if let Some(sender) = sender {
            let _ = sender.try_send(entry);
        } else {
            info!("no subscribers for unit {}, dropping log entry", unit);
        }
    }
}

/// Seek to the end of the journal,
/// `journal.seek_tail()` is not working for some reason,
/// so we use `journal.seek_realtime_usec()` with current time to achieve the same effect.
fn seek_tail(journal: &mut Journal) -> Result<()> {
    let start = SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros() as u64;

    journal.seek_realtime_usec(start)?;
    journal.next()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use anyhow::Result;
    use futures::StreamExt;
    use futures::future::try_join_all;
    use tokio::process::Command;
    use tokio::time::{sleep, timeout};

    use crate::source::LogSource;
    use crate::source::journal::JournalSource;

    /// Requires permission to run transient systemd units (usually requires root).
    ///
    /// Example:
    /// ```
    /// sudo -E RUSTFLAGS="-Awarnings" cargo test test_journal_source
    /// ```
    #[tokio::test]
    async fn test_journal_source() -> Result<()> {
        let units = ["logban_source_test1.service", "logban_source_test2.service"];
        let message = "new line";

        // start transient unit that logs some messages
        for unit in &units {
            let status = Command::new("systemd-run")
                .arg("--unit")
                .arg(unit)
                .arg("/bin/sh")
                .arg("-c")
                .arg(format!(
                    r#"sleep 1 ; for i in {{1..5}}; do echo "{} $i"; sleep 0.2; done"#,
                    message
                ))
                .status()
                .await;

            let Ok(status) = status else {
                eprintln!("systemd-run not permitted, skipping test");
                return Ok(());
            };

            if !status.success() {
                eprintln!("systemd-run failed (likely permission), skipping test");
                return Ok(());
            }
        }

        sleep(Duration::from_millis(300)).await;

        // init journal stream
        let tasks: Vec<_> = units
            .into_iter()
            .map(|unit| {
                async move {
                    let mut source = JournalSource::new(unit)?;
                    let stream = source.stream()?;

                    // look for log message
                    let found = timeout(Duration::from_secs(2), async {
                        let mut matched = false;
                        let mut stream = stream.take(3);
                        while let Some(entry) = stream.next().await {
                            matched |= entry.message.contains(message);
                            println!("Got journal entry (match={}): {}", matched, entry.message);
                        }
                        matched
                    })
                    .await
                    .unwrap_or_default();

                    anyhow::ensure!(found, "log entry not found in {}", unit);
                    Ok(())
                }
            })
            .collect();

        try_join_all(tasks).await?;

        Ok(())
    }
}
