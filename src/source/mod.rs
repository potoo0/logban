pub mod file;
pub mod journal;

use futures::stream::BoxStream;

use crate::models::LogEntry;

pub trait LogSource: Send {
    fn id(&self) -> &str;
    fn backend(&self) -> &str;
    fn stream(&mut self) -> anyhow::Result<BoxStream<'_, LogEntry>>;
}
