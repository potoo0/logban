-- Add migration script here

CREATE TABLE IF NOT EXISTS bans
(
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    ip          TEXT    NOT NULL,
    rule        TEXT    NOT NULL,
    banned_at   INTEGER NOT NULL,
    expire_at   INTEGER NOT NULL,
    is_active   BOOLEAN NOT NULL DEFAULT 1,
    unbanned_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_bans_expire_at ON bans (expire_at);
CREATE INDEX IF NOT EXISTS idx_bans_is_active ON bans (is_active);
