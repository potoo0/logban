use std::net::IpAddr;

use sqlx::FromRow;
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

#[derive(Debug, FromRow)]
pub struct BanEntity {
    pub id: Option<i64>,
    pub ip: String,
    pub rule: String,
    pub banned_at: i64,
    pub expire_at: i64,
    pub is_active: bool,
    pub unbanned_at: Option<i64>,
}

impl From<(OffsetDateTime, IpAddr)> for HitRecord {
    fn from(value: (OffsetDateTime, IpAddr)) -> Self {
        Self { timestamp: value.0, ip: value.1 }
    }
}

impl BanEntity {
    pub fn new(ip: IpAddr, rule: String, expire_at: OffsetDateTime) -> Self {
        Self {
            id: None,
            ip: ip.to_string(),
            rule,
            banned_at: OffsetDateTime::now_utc().unix_timestamp(),
            expire_at: expire_at.unix_timestamp(),
            is_active: true,
            unbanned_at: None,
        }
    }
}
