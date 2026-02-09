use anyhow::{Context, Result};
use futures::stream::BoxStream;
use futures::{StreamExt, stream};
use sdjournal::{Journal, JournalQuery};
use time::OffsetDateTime;
use tracing::warn;

use super::LogSource;
use crate::models::LogEntry;

pub struct JournalSource {
    unit: String,
    journal: JournalQuery,
}

impl JournalSource {
    pub fn new(unit: String) -> Result<Self> {
        let journal = Journal::open_default().context("Failed to open systemd journal")?;
        let mut query = journal.query();
        query.match_exact("_SYSTEMD_UNIT", unit.as_bytes());

        Ok(Self { unit, journal: query })
    }
}

impl LogSource for JournalSource {
    fn id(&self) -> &str {
        &self.unit
    }

    fn backend(&self) -> &str {
        "journal"
    }

    fn stream(&mut self) -> Result<BoxStream<'_, LogEntry>> {
        let journal = self.journal.follow_tokio()?;

        let stream = stream::unfold(journal, |mut journal| async move {
            loop {
                // read next entry
                let entry = match journal.next().await {
                    Some(Ok(entry)) => entry,
                    Some(Err(err)) => {
                        warn!("Error reading journal entry: {}", err);
                        continue;
                    }
                    None => continue,
                };

                // get MESSAGE field, skip if not present
                let message_bytes = match entry.get("MESSAGE") {
                    Some(bytes) => bytes,
                    None => continue,
                };

                // parse timestamp (UTC, microseconds → nanoseconds)
                let timestamp = OffsetDateTime::from_unix_timestamp_nanos(
                    (entry.realtime_usec() as i128) * 1_000,
                )
                .unwrap_or_else(|err| {
                    warn!("Error parsing timestamp: {}", err);
                    OffsetDateTime::now_utc()
                });

                let log_entry = LogEntry {
                    timestamp,
                    message: String::from_utf8_lossy(message_bytes).into_owned(),
                };

                return Some((log_entry, journal));
            }
        });

        Ok(stream.boxed())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use anyhow::Result;
    use futures::StreamExt;
    use tokio::process::Command;
    use tokio::time::{sleep, timeout};

    use crate::source::LogSource;
    use crate::source::journal::JournalSource;

    /// Requires permission to run transient systemd units (usually requires root).
    ///
    /// Example:
    /// ```
    /// sudo -E RUSTFLAGS="-Awarnings" cargo test test_journal_query
    /// ```
    #[tokio::test]
    async fn test_journal_query() -> Result<()> {
        let unit = "journal_source_test.service";
        let message = "new line";

        // start transient unit that logs some messages
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

        sleep(Duration::from_millis(300)).await;

        // init journal stream
        let mut source = JournalSource::new(unit.to_string())?;
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

        assert!(found, "log from transient unit not found");
        Ok(())
    }
}
