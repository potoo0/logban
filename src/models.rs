use std::net::IpAddr;

use time::OffsetDateTime;

// TODO optimize: LogEntry to avoid String allocation: use Cow<'a, str> or Bytes
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: OffsetDateTime,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct HitRecord {
    pub timestamp: OffsetDateTime,
    pub ip: IpAddr,
}

impl From<(OffsetDateTime, IpAddr)> for HitRecord {
    fn from(value: (OffsetDateTime, IpAddr)) -> Self {
        Self { timestamp: value.0, ip: value.1 }
    }
}
