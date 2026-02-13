use std::path::Path;
use anyhow::Result;
use sqlx::SqlitePool;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{
    SqliteAutoVacuum, SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
};
use tokio::fs::create_dir_all;
use crate::models::BanEntity;

static MIGRATOR: Migrator = sqlx::migrate!();

pub struct Store {
    pool: SqlitePool,
}

impl Store {
    /// Initialize the SQLite database and ensure tables exist.
    ///
    /// - Creates the database file if it does not exist.
    /// - Sets WAL journal mode and NORMAL synchronous mode for safe and performant writes.
    /// - Enables incremental auto-vacuum to manage file size.
    /// - Runs all pending migrations to create/update tables as needed.
    pub async fn new(filename: &str) -> Result<Self> {
        // create directory if needed
        if let Some(parent) = Path::new(filename).parent() {
            create_dir_all(parent).await?;
        }
        let opts = SqliteConnectOptions::new()
            .filename(filename)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .auto_vacuum(SqliteAutoVacuum::Incremental);

        let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?;
        MIGRATOR.run(&pool).await?;

        Ok(Self { pool })
    }

    pub async fn insert_active_ban(&self, record: BanEntity) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO bans (ip, rule, banned_at, expire_at, is_active, unbanned_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
            record.ip,
            record.rule,
            record.banned_at,
            record.expire_at,
            record.is_active,
            record.unbanned_at
        )
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn fetch_expired_bans(&self, ts: i64, batch_size: i32) -> Result<Vec<BanEntity>> {
        let rows = sqlx::query_as!(
            BanEntity,
            r#"
            SELECT id, ip, rule, banned_at, expire_at, is_active, unbanned_at
            FROM bans
            WHERE is_active = 1 AND expire_at <= ?
            LIMIT ?
            "#,
            ts,
            batch_size
        )
            .fetch_all(&self.pool)
            .await?;

        Ok(rows)
    }

    pub async fn mark_bans_inactive(&self, ts: i64, ids: Vec<i64>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        // build SQL
        let placeholders = std::iter::repeat_n("?", ids.len()).collect::<Vec<_>>().join(", ");
        let sql = format!(
            r#"
        UPDATE bans
        SET is_active = 0,
            unbanned_at = ?
        WHERE id IN ({})
        "#,
            placeholders
        );

        // bind params
        let mut query = sqlx::query(&sql).bind(ts);
        for id in ids {
            query = query.bind(id);
        }

        // execute the query
        query.execute(&self.pool).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use time::OffsetDateTime;
    use super::*;

    #[tokio::test]
    async fn test_store() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let db_path = dir.path().join("logban.db");
        let db_path = db_path.to_string_lossy();
        let store = Store::new(db_path.as_ref()).await?;

        let now = OffsetDateTime::now_utc();
        let count: usize = 9;
        // insert expired bans
        for offset in 0..count {
            let entity = BanEntity::new(
                "12.1.1.1".parse()?,
                "sshd".into(),
                now - Duration::from_secs((10 + offset) as u64),
            );
            store.insert_active_ban(entity).await?;
        }

        // fetch expired bans
        let expired = store.fetch_expired_bans(now.unix_timestamp(), 10).await?;
        assert_eq!(count, expired.len());

        // mark the expired ban as inactive
        let ids: Vec<i64> = expired.into_iter().filter_map(|e| e.id).collect();
        store.mark_bans_inactive(now.unix_timestamp(), ids).await?;

        // fetch again, should be empty
        let expired = store.fetch_expired_bans(now.unix_timestamp(), 10).await?;
        assert!(expired.is_empty());

        Ok(())
    }
}