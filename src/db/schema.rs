//! Connection settings and the forward-only schema migrations
//! (`PRAGMA user_version`).

use rusqlite::{Connection, TransactionBehavior};

/// Entry `n` upgrades `user_version` n to n + 1. Never edit an entry that has
/// shipped; append a new one.
///
/// Metric values are stored as scaled integers (see `SysMetric::scale`);
/// `*_x` columns hold the bucket maximum, `n` the number of raw samples. Containers are keyed by name
/// (`ctr_names`), because ids change on every recreate.
pub(super) const MIGRATIONS: &[&str] = &[
    // v1
    "
CREATE TABLE sys_samples (
  ts INTEGER PRIMARY KEY,
  cpu INTEGER, load1 INTEGER, mem INTEGER, swap INTEGER,
  disk INTEGER, net_rx INTEGER, net_tx INTEGER, temp INTEGER
);
CREATE TABLE sys_roll (
  res INTEGER NOT NULL, ts INTEGER NOT NULL,
  cpu INTEGER, cpu_x INTEGER, load1 INTEGER, load1_x INTEGER,
  mem INTEGER, mem_x INTEGER, swap INTEGER, swap_x INTEGER,
  disk INTEGER, disk_x INTEGER, net_rx INTEGER, net_rx_x INTEGER,
  net_tx INTEGER, net_tx_x INTEGER, temp INTEGER, temp_x INTEGER,
  PRIMARY KEY (res, ts)
) WITHOUT ROWID;
CREATE TABLE ctr_names (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE
);
CREATE TABLE ctr_samples (
  cid INTEGER NOT NULL REFERENCES ctr_names(id),
  ts INTEGER NOT NULL,
  cpu INTEGER, mem INTEGER,
  PRIMARY KEY (cid, ts)
) WITHOUT ROWID;
CREATE TABLE ctr_roll (
  cid INTEGER NOT NULL REFERENCES ctr_names(id),
  res INTEGER NOT NULL, ts INTEGER NOT NULL,
  cpu INTEGER, cpu_x INTEGER, mem INTEGER, mem_x INTEGER,
  PRIMARY KEY (cid, res, ts)
) WITHOUT ROWID;
CREATE TABLE events (
  id INTEGER PRIMARY KEY,
  ts INTEGER NOT NULL,
  kind TEXT NOT NULL, level TEXT NOT NULL,
  subject TEXT NOT NULL, detail TEXT NOT NULL
);
CREATE INDEX events_subject ON events (subject, id);
CREATE TABLE config_revisions (
  id INTEGER PRIMARY KEY,
  ts INTEGER NOT NULL,
  author TEXT NOT NULL, note TEXT NOT NULL, text TEXT NOT NULL
);
CREATE TABLE sessions (
  token_hash TEXT PRIMARY KEY,
  created INTEGER NOT NULL, expires INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE kv (
  key TEXT PRIMARY KEY, value TEXT NOT NULL
) WITHOUT ROWID;
",
    // v2: how many raw samples are behind a rollup row, so averages over
    // rows of different resolutions (and 1 min -> 10 min) are weighted.
    "
ALTER TABLE sys_roll ADD COLUMN n INTEGER NOT NULL DEFAULT 1;
ALTER TABLE ctr_roll ADD COLUMN n INTEGER NOT NULL DEFAULT 1;
",
    // v3: log level counts (src/db/logcounts.rs)
    super::logcounts::MIGRATION,
    // v4: extra container metrics (src/db/ctrx.rs)
    super::ctrx::MIGRATION,
    // v5: extra host series (src/db/hostx.rs)
    super::hostx::MIGRATION,
    // v6: time indexes on events. Restart counts read only the starts in
    // their window (covering index), History, Host markers, health strips
    // and retention read a time range instead of walking ids. An older build
    // refuses a database at this version (see `migrate`).
    "
CREATE INDEX events_ts ON events (ts);
CREATE INDEX events_kind_ts ON events (kind, ts, subject);
",
    // v7: accounts and database-backed dashboard configuration.
    "
INSERT INTO kv (key, value) SELECT 'auth.onboarding_locked', '1'
  FROM kv WHERE key = 'auth.password';
DELETE FROM kv WHERE key = 'auth.password';
DROP TABLE sessions;
CREATE TABLE users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL COLLATE NOCASE UNIQUE,
  password_hash TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'viewer')),
  disabled INTEGER NOT NULL DEFAULT 0 CHECK (disabled IN (0, 1)),
  scoped INTEGER NOT NULL DEFAULT 0 CHECK (scoped IN (0, 1)),
  created INTEGER NOT NULL,
  updated INTEGER NOT NULL,
  CHECK (role != 'owner' OR (disabled = 0 AND scoped = 0))
);
CREATE UNIQUE INDEX users_one_owner ON users (role) WHERE role = 'owner';
CREATE TRIGGER users_keep_owner BEFORE DELETE ON users
WHEN OLD.role = 'owner' BEGIN SELECT RAISE(ABORT, 'cannot delete owner'); END;
CREATE TRIGGER users_keep_owner_role BEFORE UPDATE OF role ON users
WHEN OLD.role = 'owner' AND NEW.role != 'owner'
BEGIN SELECT RAISE(ABORT, 'cannot demote owner'); END;
CREATE TABLE sessions (
  token_hash TEXT PRIMARY KEY,
  id TEXT NOT NULL UNIQUE DEFAULT (lower(hex(randomblob(16)))),
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created INTEGER NOT NULL,
  expires INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX sessions_user ON sessions (user_id);
CREATE TRIGGER users_revoke_disabled AFTER UPDATE OF disabled ON users
WHEN NEW.disabled = 1 BEGIN DELETE FROM sessions WHERE user_id = NEW.id; END;
CREATE TABLE access_grants (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('board', 'integration')),
  target_id TEXT NOT NULL,
  PRIMARY KEY (user_id, kind, target_id)
) WITHOUT ROWID;
CREATE TABLE invites (
  id TEXT PRIMARY KEY NOT NULL DEFAULT (lower(hex(randomblob(16)))),
  token_hash TEXT NOT NULL UNIQUE,
  role TEXT NOT NULL CHECK (role IN ('admin', 'viewer')),
  scopes TEXT CHECK (scopes IS NULL OR json_valid(scopes)),
  created INTEGER NOT NULL,
  expires INTEGER NOT NULL,
  used_at INTEGER,
  revoked_at INTEGER
);
CREATE TABLE boards (
  id TEXT PRIMARY KEY NOT NULL DEFAULT (lower(hex(randomblob(16)))),
  name TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 0,
  created INTEGER NOT NULL,
  updated INTEGER NOT NULL
);
CREATE TABLE integrations (
  id TEXT PRIMARY KEY NOT NULL DEFAULT (lower(hex(randomblob(16)))),
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  origin TEXT NOT NULL,
  config TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(config)),
  secret TEXT,
  secret_origin TEXT,
  CHECK ((secret IS NULL AND secret_origin IS NULL)
      OR (secret IS NOT NULL AND secret_origin IS NOT NULL AND secret_origin = origin))
);
CREATE TABLE board_items (
  id TEXT PRIMARY KEY NOT NULL DEFAULT (lower(hex(randomblob(16)))),
  board_id TEXT NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  integration_id TEXT REFERENCES integrations(id) ON DELETE SET NULL,
  kind TEXT NOT NULL,
  config TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(config)),
  layout TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(layout))
);
CREATE INDEX board_items_board ON board_items (board_id);
CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL CHECK (json_valid(value))
) WITHOUT ROWID;
CREATE TABLE drafts (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  value TEXT NOT NULL CHECK (json_valid(value)),
  updated INTEGER NOT NULL,
  PRIMARY KEY (user_id, key)
) WITHOUT ROWID;
",
    "
ALTER TABLE boards ADD COLUMN icon TEXT;
ALTER TABLE boards ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
ALTER TABLE boards ADD COLUMN sections TEXT NOT NULL DEFAULT '[]'
  CHECK (json_valid(sections) AND json_type(sections) = 'array');
",
    "
CREATE TABLE setup_credentials (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  item_id TEXT NOT NULL,
  origin TEXT NOT NULL,
  secret TEXT NOT NULL,
  PRIMARY KEY (user_id, item_id)
) WITHOUT ROWID;
",
    "ALTER TABLE users ADD COLUMN board_tour_seen INTEGER NOT NULL DEFAULT 0;",
    "
CREATE TABLE provider_widgets (
  id TEXT PRIMARY KEY NOT NULL DEFAULT ('provider:' || lower(hex(randomblob(16)))),
  integration_id TEXT NOT NULL REFERENCES integrations(id) ON DELETE CASCADE,
  widget TEXT NOT NULL,
  options TEXT NOT NULL CHECK (json_valid(options)),
  UNIQUE (integration_id, widget)
);
",
    "
CREATE UNIQUE INDEX integrations_id_origin ON integrations(id,origin);
CREATE TABLE integration_credentials (
  integration_id TEXT NOT NULL,
  field TEXT NOT NULL,
  origin TEXT NOT NULL,
  secret TEXT NOT NULL,
  PRIMARY KEY (integration_id, field),
  FOREIGN KEY (integration_id, origin) REFERENCES integrations(id,origin) ON DELETE CASCADE
) WITHOUT ROWID;
INSERT INTO integration_credentials (integration_id,field,origin,secret)
  SELECT id,'secret',secret_origin,secret FROM integrations WHERE secret IS NOT NULL;
UPDATE integrations SET secret=NULL,secret_origin=NULL WHERE secret IS NOT NULL;
",
    "
ALTER TABLE setup_credentials RENAME TO setup_credentials_before_fields;
CREATE TABLE setup_credentials (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  item_id TEXT NOT NULL,
  field TEXT NOT NULL,
  origin TEXT NOT NULL,
  secret TEXT NOT NULL,
  PRIMARY KEY (user_id, item_id, field)
) WITHOUT ROWID;
INSERT INTO setup_credentials (user_id,item_id,field,origin,secret)
  SELECT user_id,item_id,'secret',origin,secret FROM setup_credentials_before_fields;
DROP TABLE setup_credentials_before_fields;
",
    "
ALTER TABLE provider_widgets RENAME TO provider_widgets_before_placements;
CREATE TABLE provider_widgets (
  id TEXT PRIMARY KEY NOT NULL,
  integration_id TEXT NOT NULL REFERENCES integrations(id) ON DELETE CASCADE,
  board_id TEXT NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  widget TEXT NOT NULL,
  options TEXT NOT NULL CHECK (json_valid(options))
);
CREATE INDEX provider_widgets_integration ON provider_widgets(integration_id, widget);
CREATE INDEX provider_widgets_board ON provider_widgets(board_id);
INSERT INTO provider_widgets (id,integration_id,board_id,widget,options)
  SELECT 'provider:' || item.id, pw.integration_id, item.board_id, pw.widget, pw.options
  FROM board_items item JOIN provider_widgets_before_placements pw
    ON item.kind='widget' AND pw.id=json_extract(item.config,'$.widget_id');
UPDATE board_items SET config=json_set(config,'$.widget_id','provider:' || id)
  WHERE kind='widget' AND EXISTS (
    SELECT 1 FROM provider_widgets WHERE provider_widgets.id='provider:' || board_items.id);
DROP TABLE provider_widgets_before_placements;
",
];

/// Pragmas for every connection. `file` is false for `:memory:`.
pub(super) fn configure(conn: &Connection, file: bool) -> rusqlite::Result<()> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    // Only takes effect while the database has no tables yet (a new file);
    // it lets the weekly `incremental_vacuum` actually shrink the file.
    conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL;")?;
    if file {
        let mode: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "journal_mode is {mode}, not wal"
            )));
        }
    }
    // 2 MiB page cache, no mmap (RSS budget), temp tables in memory (the
    // container's root file system is read-only; only /data is writable).
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -2048;
         PRAGMA mmap_size = 0;
         PRAGMA journal_size_limit = 4194304;
         PRAGMA wal_autocheckpoint = 500;
         PRAGMA temp_store = MEMORY;
         PRAGMA foreign_keys = ON;
         PRAGMA recursive_triggers = ON;",
    )
}

pub(super) fn version(conn: &Connection) -> Result<usize, String> {
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .map(|v| v.max(0) as usize)
        .map_err(|e| e.to_string())
}

/// Apply every migration this build knows and the file does not have yet.
/// Idempotent; refuses a file written by a newer build.
pub(super) fn migrate(conn: &mut Connection) -> Result<(), String> {
    let e = |e: rusqlite::Error| e.to_string();
    let v = version(conn)?;
    if v > MIGRATIONS.len() {
        return Err(format!(
            "database schema version {v} is newer than this build understands ({})",
            MIGRATIONS.len()
        ));
    }
    if v == MIGRATIONS.len() {
        return Ok(());
    }
    // Immediate: another process (`set-password`) may be opening the same
    // file at the same moment; re-read the version inside the lock.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(e)?;
    let v = version(&tx)?;
    for (n, sql) in MIGRATIONS.iter().enumerate().skip(v) {
        tx.execute_batch(sql).map_err(e)?;
        tx.pragma_update(None, "user_version", (n + 1) as i64)
            .map_err(e)?;
    }
    tx.commit().map_err(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_migration_discards_credentials_and_preserves_other_settings() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn, false).unwrap();
        for sql in &MIGRATIONS[..6] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute_batch(
            "PRAGMA user_version = 6;
             INSERT INTO kv VALUES ('auth.password', 'old'), ('theme', 'dark');
             INSERT INTO sessions VALUES ('old', 0, 1);",
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        let count = |sql| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM sessions"), 0);
        assert_eq!(count("SELECT COUNT(*) FROM kv WHERE key = 'theme'"), 1);
        assert_eq!(
            count("SELECT COUNT(*) FROM kv WHERE key = 'auth.password'"),
            0
        );
        assert_eq!(count("SELECT COUNT(*) FROM boards"), 0);
        assert_eq!(
            count("SELECT COUNT(*) FROM kv WHERE key = 'auth.onboarding_locked'"),
            1
        );
        conn.execute(
            "INSERT INTO users (name, password_hash, role, created, updated)
                      VALUES ('alice', 'hash', 'owner', 1, 1)",
            [],
        )
        .unwrap();
        for sql in [
            "INSERT INTO users (name, password_hash, role, created, updated)
             VALUES ('bob', 'hash', 'owner', 1, 1)",
            "UPDATE users SET role = 'admin'",
            "UPDATE users SET disabled = 1",
            "DELETE FROM users",
            "INSERT OR REPLACE INTO users (name, password_hash, role, created, updated)
             VALUES ('alice', 'newhash', 'viewer', 2, 2)",
        ] {
            assert!(conn.execute(sql, []).is_err(), "{sql}");
        }
    }

    #[test]
    fn provider_upgrade_preserves_existing_http_connections_and_board_targets() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn, false).unwrap();
        let previous = MIGRATIONS
            .iter()
            .position(|sql| sql.contains("CREATE TABLE integration_credentials"))
            .unwrap();
        for sql in &MIGRATIONS[..previous] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", previous as i64)
            .unwrap();
        conn.execute_batch(
            r#"INSERT INTO integrations (id,name,kind,origin,config,secret,secret_origin)
                VALUES ('legacy','Old HTTP','http','https://fixture.test',
                        '{"base_url":"https://fixture.test/status","enabled":true}',
                        'fake-bearer','https://fixture.test');
               INSERT INTO boards (id,name,created,updated) VALUES ('old-board','Board',1,1);
               INSERT INTO board_items (id,board_id,kind,config,integration_id) VALUES
                ('old-link','old-board','integration','{"kind":"integration","integration_id":"legacy"}','legacy'),
                ('old-widget','old-board','widget','{"kind":"widget","widget_id":"local-toml"}',NULL);
               INSERT INTO boards (id,name,created,updated) VALUES ('other-board','Other',1,1);
               INSERT INTO provider_widgets(id,integration_id,widget,options)
                   VALUES ('provider:old','legacy','json','{"paths":["value"],"refresh":30}');
               INSERT INTO board_items(id,board_id,kind,config) VALUES
                   ('provider-a','old-board','widget','{"kind":"widget","widget_id":"provider:old"}'),
                   ('provider-b','other-board','widget','{"kind":"widget","widget_id":"provider:old"}');"#,
        ).unwrap();
        migrate(&mut conn).unwrap();
        let saved = conn
            .query_row(
                "SELECT json_extract(config,'$.base_url'), c.secret, c.origin
             FROM integrations JOIN integration_credentials c ON integration_id=id
             WHERE id='legacy' AND field='secret' ",
                [],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            saved,
            (
                "https://fixture.test/status".into(),
                "fake-bearer".into(),
                "https://fixture.test".into()
            )
        );
        let targets: String = conn.query_row(
            "SELECT group_concat(id) FROM (SELECT id FROM board_items WHERE board_id='old-board' ORDER BY id)",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(targets, "old-link,old-widget,provider-a");
        let placements: i64 = conn.query_row(
            "SELECT count(*) FROM provider_widgets pw JOIN board_items item
             ON item.board_id=pw.board_id AND json_extract(item.config,'$.widget_id')=pw.id
             WHERE pw.integration_id='legacy' AND pw.widget='json'
               AND json_extract(pw.options,'$.paths[0]')='value' AND json_extract(pw.options,'$.refresh')=30",
             [], |row| row.get(0)).unwrap();
        assert_eq!(placements, 2);
    }

    #[test]
    fn boards_start_empty_and_integration_secrets_are_origin_bound() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn, false).unwrap();
        migrate(&mut conn).unwrap();
        let new_board = || {
            conn.query_row(
                "INSERT INTO boards (name, created, updated) VALUES ('Blank', 1, 1)
                 RETURNING id, version",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .unwrap()
        };
        let (board, version) = new_board();
        assert_eq!(board.len(), 32);
        assert_eq!(version, 0);
        assert_ne!(board, new_board().0);
        let items: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM board_items WHERE board_id = ?1",
                [&board],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(items, 0);
        let integration: String = conn.query_row(
            "INSERT INTO integrations (name,kind,origin) VALUES ('Example','http','https://example.test')
             RETURNING id", [], |row| row.get(0)).unwrap();
        conn.execute(
            "INSERT INTO integration_credentials (integration_id,field,origin,secret)
                      VALUES (?1,'secret','https://example.test','fixture')",
            [&integration],
        )
        .unwrap();
        assert_eq!(integration.len(), 32);
        assert!(conn
            .execute(
                "UPDATE integrations SET origin = 'https://new.example.test' WHERE id = ?1",
                [&integration],
            )
            .is_err());
        assert!(conn
            .execute(
                "INSERT INTO integration_credentials (integration_id,field,origin,secret)
                      VALUES (?1,'other','https://wrong.test','fixture')",
                [&integration]
            )
            .is_err());
        conn.execute(
            "DELETE FROM integration_credentials WHERE integration_id=?1",
            [&integration],
        )
        .unwrap();
        assert_eq!(
            conn.execute(
                "UPDATE integrations SET origin='https://new.example.test' WHERE id=?1",
                [&integration]
            )
            .unwrap(),
            1
        );
    }
}
