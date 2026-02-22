use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::Result;
use futures::stream::BoxStream;
use futures::{StreamExt, stream};
use inotify::{EventMask, Inotify, WatchMask};
use time::OffsetDateTime;
use tokio::fs::{File, metadata};
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader, SeekFrom};
use tokio::time::sleep;
use tracing::{info, trace, warn};

use super::LogSource;
use crate::models::LogEntry;

pub struct FileSource {
    path: String,
    /// 兜底轮询间隔：当 inotify 没捕捉到（或权限问题）时，仍会周期性重试 open/read
    poll_interval: Duration,
}

impl FileSource {
    pub fn new(path: String) -> Result<Self> {
        Ok(Self { path, poll_interval: Duration::from_millis(1000) })
    }

    #[allow(dead_code)]
    pub fn with_poll_interval(mut self, d: Duration) -> Self {
        self.poll_interval = d;
        self
    }
}

impl LogSource for FileSource {
    fn id(&self) -> &str {
        &self.path
    }

    fn backend(&self) -> &str {
        "file"
    }

    fn stream(&mut self) -> Result<BoxStream<'_, LogEntry>> {
        let state = TailState::new(&self.path, self.poll_interval);

        let stream = stream::unfold(state, |mut st| async move {
            loop {
                // 1. wait for file to be available
                st.wait_for_reader().await;

                // 2. try to read exists line first
                let reader = st.reader.as_mut().unwrap();
                st.line.clear();
                match reader.read_line(&mut st.line).await {
                    Ok(n) if n > 0 => {
                        let entry = LogEntry {
                            timestamp: OffsetDateTime::now_utc(),
                            message: st.line.trim().into(),
                        };
                        return Some((entry, st));
                    }
                    Ok(_) => {
                        // EOF, wait for file change
                        if st.maybe_rewind().await {
                            continue;
                        }
                    }
                    Err(e) => {
                        warn!("error reading file: {}, resetting reader", e);
                        st.reset();
                        continue;
                    }
                }

                // 3. wait for inotify events
                if st.wait_for_reset_event().await {
                    info!("file changed (moved/deleted), resetting reader");
                    st.reset();
                }
            }
        });

        Ok(stream.boxed())
    }
}

struct TailState {
    path: PathBuf,
    poll_interval: Duration,

    reader: Option<BufReader<File>>,
    inotify: Option<AsyncFd<Inotify>>,

    event_buf: [u8; 1024],
    line: String,
    last_change: Option<SystemTime>,
}

impl TailState {
    fn new(path: impl Into<PathBuf>, poll_interval: Duration) -> Self {
        Self {
            path: path.into(),
            poll_interval,
            reader: None,
            inotify: None,
            event_buf: [0; 1024],
            line: String::new(),
            last_change: None,
        }
    }

    /// Reset to inactive state
    fn reset(&mut self) {
        self.reader = None;
        self.inotify = None;
    }

    /// Init reader and inotify watcher.
    /// Return false if the file cannot be read yet
    async fn wait_for_reader(&mut self) {
        if self.reader.is_some() {
            return;
        }

        loop {
            match File::open(&self.path).await {
                Ok(file) => {
                    let mut reader = BufReader::new(file);
                    if reader.seek(SeekFrom::End(0)).await.is_err() {
                        sleep(self.poll_interval).await;
                        continue;
                    }

                    match Inotify::init()
                        .and_then(|ino| {
                            let mask = WatchMask::MODIFY
                                | WatchMask::MOVE_SELF
                                | WatchMask::DELETE_SELF
                                | WatchMask::ATTRIB;
                            ino.watches().add(&self.path, mask)?;
                            Ok(ino)
                        })
                        .and_then(AsyncFd::new)
                    {
                        Ok(fd) => {
                            info!("started watching file");
                            self.reader = Some(reader);
                            self.inotify = Some(fd);
                            return;
                        }
                        Err(_) => {
                            sleep(self.poll_interval).await;
                        }
                    }
                }
                Err(_) => {
                    trace!("waiting for file to be created");
                    // file does not exist / permission denied, retry later
                    sleep(self.poll_interval).await;
                }
            }
        }
    }

    /// Wait for inotify events and determine whether a reset is required.
    ///
    /// This function blocks until at least one inotify event is received.
    ///
    /// Returns:
    /// - `true`  if the event indicates the file has been moved, deleted, or otherwise replaced,
    ///   requiring the reader to be reset.
    /// - `false` if only non-destructive events were observed.
    async fn wait_for_reset_event(&mut self) -> bool {
        let async_fd = match self.inotify.as_mut() {
            Some(fd) => fd,
            None => return false,
        };

        loop {
            // `read_events` requires `&mut self`, we take it from the AsyncFd directly.
            let inotify = async_fd.get_mut();
            match inotify.read_events(&mut self.event_buf) {
                Ok(events) => {
                    for ev in events {
                        trace!("got inotify event: {:?}", ev.mask);
                        if ev.mask.contains(EventMask::MOVE_SELF)
                            || ev.mask.contains(EventMask::DELETE_SELF)
                            || ev.mask.contains(EventMask::IGNORED)
                        {
                            return true;
                        }
                        if ev.mask.contains(EventMask::ATTRIB) {
                            match metadata(&self.path).await {
                                Ok(new) => {
                                    let old = match self.reader.as_ref() {
                                        Some(reader) => match reader.get_ref().metadata().await {
                                            Ok(meta) => meta,
                                            Err(_) => return true,
                                        },
                                        None => return true,
                                    };

                                    if old.ino() != new.ino() || old.dev() != new.dev() {
                                        info!(
                                            "file identity changed (inode/dev mismatch), treating as replaced"
                                        );
                                        return true;
                                    }
                                }
                                Err(_) => {
                                    info!("file meta inaccessible, treating as deleted");
                                    return true;
                                }
                            }
                        }
                    }
                    return false;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    trace!("got WouldBlock, waiting...");
                    // Wait until fd becomes readable
                    let mut guard = match async_fd.readable().await {
                        Ok(g) => g,
                        Err(_) => return true,
                    };
                    // Spurious readiness, clear and wait again
                    guard.clear_ready();
                }
                Err(e) => {
                    warn!("read events error: {}", e);
                    // Real IO error, force reset
                    return true;
                }
            }
        }
    }

    /// Rewind reader to start if file was truncated or rewritten.
    /// Returns true if rewind happened.
    async fn maybe_rewind(&mut self) -> bool {
        let reader = match self.reader.as_mut() {
            Some(r) => r,
            None => return false,
        };

        let meta = match metadata(&self.path).await {
            Ok(m) => m,
            Err(_) => return false,
        };

        let offset = match reader.stream_position().await {
            Ok(p) => p,
            Err(_) => return false,
        };

        // classic truncate
        if meta.len() < offset {
            info!("file truncated, rewinding to start");
            self.last_change = Self::meta_change_time(&meta);
            let _ = reader.rewind().await;
            return true;
        }
        false

        // // rewrite: size == offset, but metadata changed.
        // //
        // // Scenario:
        // //   echo 1 > file
        // //   echo 2 > file
        // //
        // //   The file is truncated and rewritten to the same size.
        // //   From tail's view: inode unchanged, size unchanged, offset unchanged.
        // //   Only metadata (mtime/ctime) changes, which signals a logical content reset.
        // let Some(change) = Self::meta_change_time(&meta) else {
        //     return false;
        // };
        // // FIXME Scenario echo 1 >> file
        // let rewritten = self.last_change.is_some_and(|last| change > last);
        // self.last_change = Some(change);
        // // if rewritten {
        // //     info!("file rewritten, rewinding to start");
        // //     let _ = reader.seek(SeekFrom::Start(0)).await;
        // // }
        //
        // rewritten
    }

    fn meta_change_time(meta: &std::fs::Metadata) -> Option<SystemTime> {
        meta.modified().ok().or_else(|| meta.created().ok())
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{OpenOptions, rename};
    use std::io::Write;

    use tempfile::NamedTempFile;
    use tokio::sync::mpsc;
    use tokio::time::{Duration, timeout};

    use super::*;

    #[tokio::test]
    async fn test_tail() -> Result<()> {
        let mut context = init_test_context()?;

        // Create the tail stream
        let mut stream = context.source.stream()?;
        // Trigger the first poll so the stream opens the file and seeks to end
        let _ = timeout(Duration::from_millis(10), stream.next()).await;

        // Write multiple lines at once
        writeln!(context.file, "new line 1")?;
        writeln!(context.file, "new line 2")?;
        context.file.flush()?;

        expect_line(&mut stream, "new line 1").await?;
        expect_line(&mut stream, "new line 2").await?;

        // Write one line at a time and immediately read it
        write_and_expect_line(&mut context.file, &mut stream, "new line 3").await?;
        write_and_expect_line(&mut context.file, &mut stream, "new line 4").await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_tail_wait_line() -> Result<()> {
        let mut context = init_test_context()?;

        // Create the tail stream
        let mut stream = context.source.stream().expect("failed to create stream");
        // Trigger the first poll so the stream opens the file and seeks to end
        let _ = timeout(Duration::from_millis(10), stream.next()).await;

        // Read no more lines
        let res = timeout(Duration::from_secs(1), stream.next()).await;
        assert!(res.is_err(), "expected timeout, but got a line");

        // Wait for new lines to be written
        let lines = ["new line 1", "new line 2"];
        let (tx, mut rx) = mpsc::channel::<()>(1);
        let reader = async {
            let mut results = Vec::new();
            for _ in 0..lines.len() {
                // Notify the main task that the reader is ready for the next line
                tx.send(()).await.unwrap();

                // Wait for the next line to be available
                let entry = timeout(Duration::from_secs(1), stream.next())
                    .await
                    .expect("timeout waiting for new line")
                    .expect("stream ended unexpectedly");

                results.push(entry.message);
            }

            results
        };

        let writer = async {
            for expected_line in lines {
                // wait for reader to be ready for the next line
                rx.recv().await.expect("reader never started");
                // simulate some delay before writing
                sleep(Duration::from_millis(300)).await;

                writeln!(context.file, "{expected_line}")?;
                context.file.flush()?;
            }
            Ok::<(), anyhow::Error>(())
        };

        let (actual_lines, _) = tokio::join!(reader, writer);

        // Verify reader got the correct lines
        for (actual, expected) in actual_lines.iter().zip(lines) {
            assert_eq!(actual.as_ref(), expected);
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_tail_wait_file() -> Result<()> {
        async fn create_and_expect_line(
            path: &str, stream: &mut BoxStream<'_, LogEntry>,
        ) -> Result<()> {
            // create the file and write a line after a delay
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("failed to open file for writing");
            sleep(Duration::from_millis(300)).await;
            info!("file created");

            // Read no more lines, since the stream should have seeked to end
            let res = timeout(Duration::from_secs(1), stream.next()).await;
            assert!(res.is_err(), "expected timeout, but got a line");

            // Write one line at a time and immediately read it
            write_and_expect_line(&mut file, stream, "new line 1 after create").await?;
            write_and_expect_line(&mut file, stream, "new line 2 after create").await
        }

        let context = init_test_context()?;
        drop(context.file);

        // Create the tail stream with a shorter poll interval for the test
        let mut source = context.source.with_poll_interval(Duration::from_millis(100));
        // Create the tail stream
        let mut stream = source.stream().expect("failed to create stream");
        // Trigger the first poll so the stream opens the file and seeks to end
        let _ = timeout(Duration::from_millis(10), stream.next()).await;

        // Read no more lines
        let res = timeout(Duration::from_secs(1), stream.next()).await;
        assert!(res.is_err(), "expected timeout, but got a line");

        create_and_expect_line(&context.path, &mut stream).await?;

        // drop the file and expect no more lines
        sleep(Duration::from_millis(100)).await;
        let res = timeout(Duration::from_secs(1), stream.next()).await;
        assert!(res.is_err(), "expected timeout, but got a line");

        // recreate the file and write lines
        create_and_expect_line(&context.path, &mut stream).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_tail_truncate() -> Result<()> {
        let mut context = init_test_context()?;

        // Create the tail stream
        let mut stream = context.source.stream()?;
        // Trigger the first poll so the stream opens the file and seeks to end
        let _ = timeout(Duration::from_millis(10), stream.next()).await;

        // Truncate the file and write new content
        context.file.as_file_mut().set_len(0)?;

        // Read lines after truncate
        write_and_expect_line(&mut context.file, &mut stream, "new line 1 after truncate").await?;
        write_and_expect_line(&mut context.file, &mut stream, "new line 2 after truncate").await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_tail_move() -> Result<()> {
        let mut context = init_test_context()?;
        // Create the tail stream
        let mut stream = context.source.stream()?;
        // Trigger the first poll so the stream opens the file and seeks to end
        let _ = timeout(Duration::from_millis(10), stream.next()).await;

        // Write and expect lines before move
        write_and_expect_line(&mut context.file, &mut stream, "line 1 before replace").await?;
        write_and_expect_line(&mut context.file, &mut stream, "line 2 before replace").await?;

        // Move the file
        rename(context.path.clone(), context.path.clone() + ".new")?;

        // Read no more lines
        let res = timeout(Duration::from_secs(1), stream.next()).await;
        assert!(res.is_err(), "expected timeout, but got a line");

        Ok(())
    }

    async fn write_and_expect_line(
        file: &mut dyn Write, stream: &mut BoxStream<'_, LogEntry>, line: &str,
    ) -> Result<()> {
        writeln!(file, "{}", line)?;
        file.flush()?;

        expect_line(stream, line).await
    }

    async fn expect_line(stream: &mut BoxStream<'_, LogEntry>, line: &str) -> Result<()> {
        let entry = timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("timeout waiting for new line")
            .expect("stream ended unexpectedly");
        assert_eq!(entry.message.as_ref(), line);

        Ok(())
    }

    struct TestContext {
        file: NamedTempFile,
        path: String,
        source: FileSource,
    }

    fn init_test_context() -> Result<TestContext> {
        let mut file = NamedTempFile::new()?;

        writeln!(file, "# Initial Lines")?;
        file.flush()?;
        let path = file.path().to_string_lossy().into_owned();
        let source = FileSource::new(path.clone())?;

        Ok(TestContext { file, path, source })
    }
}
