//! Account and dashboard access persistence.
pub(crate) const SETUP_DRAFT_KEY: &str = "owner-setup";

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::{Db, MAX_SESSIONS};

const SESSION_MAX_AGE_SECS: i64 = 30 * 86_400;
const SLIDE_MIN_SECS: i64 = 3_600;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
#[cfg_attr(test, ts(rename_all = "lowercase"))]
pub enum Role {
    Viewer,
    Admin,
    Owner,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }

    fn read(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Self> {
        match row.get_ref(index)?.as_str()? {
            "owner" => Ok(Self::Owner),
            "admin" => Ok(Self::Admin),
            "viewer" => Ok(Self::Viewer),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct Scopes {
    #[serde(default)]
    pub boards: Vec<String>,
    #[serde(default)]
    pub integrations: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub role: Role,
    pub disabled: bool,
    pub created: i64,
    pub updated: i64,
    pub scopes: Option<Scopes>,
}

impl Account {
    fn read(conn: &Connection, row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        let id = row.get(0)?;
        Ok(Self {
            id,
            name: row.get(1)?,
            role: Role::read(row, 2)?,
            disabled: row.get(3)?,
            created: row.get(4)?,
            updated: row.get(5)?,
            scopes: if row.get(6)? {
                let mut scopes = Scopes::default();
                let mut query = conn.prepare(
                    "SELECT kind, target_id FROM access_grants WHERE user_id = ?1 ORDER BY target_id",
                )?;
                let mut rows = query.query([id])?;
                while let Some(grant) = rows.next()? {
                    let targets = match grant.get_ref(0)?.as_str()? {
                        "board" => &mut scopes.boards,
                        _ => &mut scopes.integrations,
                    };
                    targets.push(grant.get(1)?);
                }
                Some(scopes)
            } else {
                None
            },
        })
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Invite {
    pub id: String,
    pub role: Role,
    pub scopes: Option<Scopes>,
    pub created: i64,
    pub expires: i64,
    pub used_at: Option<i64>,
    pub revoked_at: Option<i64>,
}

impl Invite {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        let scopes: Option<String> = row.get(2)?;
        Ok(Self {
            id: row.get(0)?,
            role: Role::read(row, 1)?,
            scopes: scopes
                .map(|s| serde_json::from_str(&s).map_err(|_| rusqlite::Error::InvalidQuery))
                .transpose()?,
            created: row.get(3)?,
            expires: row.get(4)?,
            used_at: row.get(5)?,
            revoked_at: row.get(6)?,
        })
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionInfo {
    pub id: String,
    pub created: i64,
    pub expires: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountError {
    OnboardingLocked,
    Conflict,
    Missing,
    Protected,
    Invalid,
    InvalidScopes,
    Storage,
}

pub enum AccountChange {
    Role(Role),
    Disabled(bool),
    Scopes(Option<Scopes>),
    SignOut,
    Delete,
}

type Result<T> = std::result::Result<T, AccountError>;

const ACCOUNT: &str = "id, name, role, disabled, created, updated, scoped";
const INVITE: &str = "id, role, scopes, created, expires, used_at, revoked_at";

fn insert_user(
    conn: &Connection,
    name: &str,
    hash: &str,
    role: Role,
    now: i64,
) -> rusqlite::Result<i64> {
    conn.query_row(
        "INSERT INTO users (name, password_hash, role, created, updated)
         VALUES (?1, ?2, ?3, ?4, ?4) RETURNING id",
        params![name, hash, role.as_str(), now],
        |row| row.get(0),
    )
}

fn insert_session(
    conn: &Connection,
    user: i64,
    hash: &str,
    now: i64,
    expires: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sessions (token_hash, user_id, created, expires) VALUES (?1, ?2, ?3, ?4)",
        params![hash, user, now, expires],
    )?;
    conn.execute("DELETE FROM sessions WHERE expires <= ?1", [now])?;
    conn.execute(
        "DELETE FROM sessions WHERE token_hash IN
         (SELECT token_hash FROM sessions WHERE user_id = ?3
          ORDER BY created DESC, expires DESC, (token_hash = ?2) DESC LIMIT -1 OFFSET ?1)",
        params![MAX_SESSIONS, hash, user],
    )?;
    Ok(())
}

fn owner_id(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    conn.query_row("SELECT id FROM users WHERE role = 'owner'", [], |r| {
        r.get(0)
    })
    .optional()
}

pub(crate) fn read_accounts(conn: &Connection) -> rusqlite::Result<Vec<Account>> {
    let mut stmt = conn.prepare(&format!("SELECT {ACCOUNT} FROM users ORDER BY name"))?;
    let rows = stmt.query_map([], |r| Account::read(conn, r))?;
    rows.collect()
}

/// Revoke outstanding invitations whose promised access can no longer be granted.
pub(crate) fn revoke_target_invites(
    conn: &Connection,
    scope: &str,
    id: &str,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE invites SET revoked_at=?3
         WHERE used_at IS NULL AND revoked_at IS NULL AND expires>?3
         AND EXISTS (SELECT 1 FROM json_each(invites.scopes, ?1) WHERE value=?2)",
        params![scope, id, now],
    )?;
    Ok(())
}

fn valid_targets(conn: &Connection, scopes: Option<&Scopes>) -> rusqlite::Result<bool> {
    let Some(scopes) = scopes else {
        return Ok(true);
    };
    for (sql, ids) in [
        (
            "SELECT EXISTS (SELECT 1 FROM boards WHERE id=?1)",
            &scopes.boards,
        ),
        (
            "SELECT EXISTS (SELECT 1 FROM integrations WHERE id=?1)",
            &scopes.integrations,
        ),
    ] {
        let mut stmt = conn.prepare(sql)?;
        for id in ids {
            if !stmt.query_row([id], |r| r.get::<_, bool>(0))? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn set_scopes(
    conn: &Connection,
    user: i64,
    scopes: Option<&Scopes>,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM access_grants WHERE user_id = ?1", [user])?;
    conn.execute(
        "UPDATE users SET scoped = ?2, updated = ?3 WHERE id = ?1",
        params![user, scopes.is_some(), now],
    )?;
    if let Some(scopes) = scopes {
        let mut insert = conn.prepare(
            "INSERT OR IGNORE INTO access_grants (user_id, kind, target_id) VALUES (?1, ?2, ?3)",
        )?;
        for (kind, targets) in [
            ("board", &scopes.boards),
            ("integration", &scopes.integrations),
        ] {
            for target in targets {
                insert.execute(params![user, kind, target])?;
            }
        }
    }
    Ok(())
}

fn active_invite(conn: &Connection, hash: &str, now: i64) -> rusqlite::Result<Option<Invite>> {
    conn.query_row(
        &format!(
            "SELECT {INVITE} FROM invites WHERE token_hash = ?1 AND expires > ?2
                  AND used_at IS NULL AND revoked_at IS NULL"
        ),
        params![hash, now],
        Invite::read,
    )
    .optional()
}

fn onboarding_locked(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM kv WHERE key = 'auth.onboarding_locked')",
        [],
        |row| row.get(0),
    )
}

impl Db {
    pub fn onboarding_locked(&self) -> bool {
        self.with(false, |i| onboarding_locked(&i.conn))
    }

    pub fn owner_exists(&self) -> bool {
        self.with(false, |i| Ok(owner_id(&i.conn)?.is_some()))
    }

    pub fn create_owner(
        &self,
        name: &str,
        hash: &str,
        session: &str,
        now: i64,
        expires: i64,
    ) -> Result<()> {
        self.with(Err(AccountError::Storage), |i| {
            let tx = i
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            if owner_id(&tx)?.is_some() {
                return Ok(Err(AccountError::Conflict));
            }
            if onboarding_locked(&tx)? {
                return Ok(Err(AccountError::OnboardingLocked));
            }
            let id = insert_user(&tx, name, hash, Role::Owner, now)?;
            tx.execute(
                "INSERT INTO drafts (user_id,key,value,updated) VALUES (?1,?2,'{}',?3)",
                params![id, SETUP_DRAFT_KEY, now],
            )?;
            insert_session(&tx, id, session, now, expires)?;
            tx.commit()?;
            Ok(Ok(()))
        })
    }

    pub fn recover_owner(&self, name: Option<&str>, hash: &str, now: i64) -> Result<()> {
        self.with(Err(AccountError::Storage), |i| {
            let tx = i.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if let Some(id) = owner_id(&tx)? {
                if let Some(name) = name {
                    let taken: bool = tx.query_row(
                        "SELECT EXISTS (SELECT 1 FROM users WHERE name = ?1 AND id != ?2)",
                        params![name, id], |r| r.get(0),
                    )?;
                    if taken {
                        return Ok(Err(AccountError::Conflict));
                    }
                }
                tx.execute(
                    "UPDATE users SET name = COALESCE(?1, name), password_hash = ?2, updated = ?3 WHERE id = ?4",
                    params![name, hash, now, id],
                )?;
                tx.execute("DELETE FROM sessions WHERE user_id = ?1", [id])?;
            } else {
                insert_user(&tx, name.unwrap_or("admin"), hash, Role::Owner, now)?;
            }
            tx.execute("DELETE FROM kv WHERE key = 'auth.onboarding_locked'", [])?;
            tx.commit()?;
            Ok(Ok(()))
        })
    }

    pub fn credential(&self, name: &str) -> Option<(i64, String, bool)> {
        self.with(None, |i| {
            i.conn
                .query_row(
                    "SELECT id, password_hash, disabled FROM users WHERE name = ?1",
                    [name],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
        })
    }

    pub fn sign_in(
        &self,
        user: i64,
        password_hash: &str,
        token_hash: &str,
        now: i64,
        expires: i64,
    ) -> Result<()> {
        self.with(Err(AccountError::Storage), |i| {
            let tx = i.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let eligible: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM users WHERE id = ?1 AND password_hash = ?2 AND disabled = 0)",
                params![user, password_hash], |r| r.get(0),
            )?;
            if !eligible {
                return Ok(Err(AccountError::Invalid));
            }
            insert_session(&tx, user, token_hash, now, expires)?;
            tx.commit()?;
            Ok(Ok(()))
        })
    }

    #[cfg(test)]
    pub fn session_is_valid(&self, hash: &str, now: i64) -> bool {
        self.with(false, |i| {
            i.conn.query_row(
                "SELECT EXISTS (
                SELECT 1 FROM sessions JOIN users ON users.id = sessions.user_id
                WHERE token_hash = ?1 AND expires > ?2 AND sessions.created > ?3 AND disabled = 0
            )",
                params![hash, now, now - SESSION_MAX_AGE_SECS],
                |row| row.get(0),
            )
        })
    }

    pub fn account_session(&self, hash: &str, now: i64) -> Option<Account> {
        self.with(None, |i| {
            let query = format!(
                "SELECT {ACCOUNT} FROM users
                 JOIN (SELECT user_id FROM sessions
                       WHERE token_hash = ?1 AND expires > ?2 AND created > ?3) s
                 ON users.id = s.user_id WHERE disabled = 0"
            );
            i.conn
                .query_row(
                    &query,
                    params![hash, now, now - SESSION_MAX_AGE_SECS],
                    |row| Account::read(&i.conn, row),
                )
                .optional()
        })
    }

    pub fn slide_session(&self, hash: &str, now: i64, expires: i64) -> Option<i64> {
        self.with(None, |i| {
            i.conn
                .query_row(
                    "UPDATE sessions SET expires = min(?1, created + ?2)
                 WHERE token_hash = ?3 AND expires > ?4
                   AND expires <= ?1 - ?5 AND expires < created + ?2
                   AND created > ?4 - ?2
                 RETURNING expires",
                    params![expires, SESSION_MAX_AGE_SECS, hash, now, SLIDE_MIN_SECS],
                    |row| row.get(0),
                )
                .optional()
        })
    }

    pub fn change_account_password(
        &self,
        id: i64,
        old: &str,
        new: &str,
        keep: &str,
        now: i64,
    ) -> Result<()> {
        self.with(Err(AccountError::Storage), |i| {
            let tx = i.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let changed = tx.execute(
                "UPDATE users SET password_hash = ?2, updated = ?3
                 WHERE id = ?1 AND password_hash = ?4 AND disabled = 0
                   AND EXISTS (SELECT 1 FROM sessions WHERE user_id = ?1 AND token_hash = ?5 AND expires > ?3 AND created > ?6)",
                params![id, new, now, old, keep, now - SESSION_MAX_AGE_SECS],
            )?;
            if changed == 0 {
                return Ok(Err(AccountError::Conflict));
            }
            tx.execute("DELETE FROM sessions WHERE user_id = ?1 AND token_hash != ?2", params![id, keep])?;
            tx.commit()?;
            Ok(Ok(()))
        })
    }

    pub fn other_sessions(&self, user: i64, keep: &str, now: i64) -> Result<Vec<SessionInfo>> {
        self.with(Err(AccountError::Storage), |i| {
            let mut stmt = i.conn.prepare(
                "SELECT id, created, expires FROM sessions WHERE user_id = ?1 AND token_hash != ?2
                 AND expires > ?3 ORDER BY created DESC, id",
            )?;
            let rows = stmt.query_map(params![user, keep, now], |r| {
                Ok(SessionInfo {
                    id: r.get(0)?,
                    created: r.get(1)?,
                    expires: r.get(2)?,
                })
            })?;
            Ok(Ok(rows.collect::<rusqlite::Result<_>>()?))
        })
    }

    pub fn sign_out_other_sessions(&self, user: i64, keep: &str, id: Option<&str>) -> Result<()> {
        self.with(Err(AccountError::Storage), |i| {
            let removed = i.conn.execute(
                "DELETE FROM sessions WHERE user_id = ?1 AND token_hash != ?2 AND (?3 IS NULL OR id = ?3)",
                params![user, keep, id],
            )?;
            Ok(if id.is_some() && removed == 0 { Err(AccountError::Missing) } else { Ok(()) })
        })
    }

    #[cfg(test)]
    pub fn accounts(&self) -> Result<Vec<Account>> {
        self.with(Err(AccountError::Storage), |i| {
            Ok(Ok(read_accounts(&i.conn)?))
        })
    }

    pub fn change_account(&self, user: i64, change: AccountChange, now: i64) -> Result<()> {
        self.with(Err(AccountError::Storage), |i| {
            let tx = i
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let role = tx
                .query_row("SELECT role FROM users WHERE id = ?1", [user], |r| {
                    Role::read(r, 0)
                })
                .optional()?;
            if role.is_none() {
                return Ok(Err(AccountError::Missing));
            }
            if matches!(change, AccountChange::Role(Role::Owner))
                || (role == Some(Role::Owner) && !matches!(change, AccountChange::SignOut))
            {
                return Ok(Err(AccountError::Protected));
            }
            match change {
                AccountChange::Role(role) => {
                    tx.execute(
                        "UPDATE users SET role = ?2, updated = ?3 WHERE id = ?1",
                        params![user, role.as_str(), now],
                    )?;
                }
                AccountChange::Disabled(disabled) => {
                    tx.execute(
                        "UPDATE users SET disabled = ?2, updated = ?3 WHERE id = ?1",
                        params![user, disabled, now],
                    )?;
                }
                AccountChange::Scopes(scopes) => {
                    if !valid_targets(&tx, scopes.as_ref())? {
                        return Ok(Err(AccountError::InvalidScopes));
                    }
                    set_scopes(&tx, user, scopes.as_ref(), now)?;
                }
                AccountChange::SignOut => {
                    tx.execute("DELETE FROM sessions WHERE user_id = ?1", [user])?;
                }
                AccountChange::Delete => {
                    tx.execute("DELETE FROM users WHERE id = ?1", [user])?;
                }
            }
            tx.commit()?;
            Ok(Ok(()))
        })
    }

    pub fn create_invite(
        &self,
        hash: &str,
        role: Role,
        scopes: Option<&Scopes>,
        now: i64,
        expires: i64,
    ) -> Result<Invite> {
        self.with(Err(AccountError::Storage), |i| {
            let tx = i
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            if !valid_targets(&tx, scopes)? {
                return Ok(Err(AccountError::InvalidScopes));
            }
            let scopes = scopes
                .map(serde_json::to_string)
                .transpose()
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let invite = tx.query_row(
                &format!(
                    "INSERT INTO invites (token_hash, role, scopes, created, expires)
                 VALUES (?1, ?2, ?3, ?4, ?5) RETURNING {INVITE}"
                ),
                params![hash, role.as_str(), scopes, now, expires],
                Invite::read,
            )?;
            tx.commit()?;
            Ok(Ok(invite))
        })
    }

    pub fn invites(&self) -> Result<Vec<Invite>> {
        self.with(Err(AccountError::Storage), |i| {
            let mut stmt = i.conn.prepare(&format!(
                "SELECT {INVITE} FROM invites ORDER BY created DESC, id"
            ))?;
            let rows = stmt.query_map([], Invite::read)?;
            Ok(Ok(rows.collect::<rusqlite::Result<_>>()?))
        })
    }

    pub fn validate_invite(&self, hash: &str, now: i64) -> Option<Invite> {
        self.with(None, |i| {
            let Some(invite) = active_invite(&i.conn, hash, now)? else {
                return Ok(None);
            };
            Ok(valid_targets(&i.conn, invite.scopes.as_ref())?.then_some(invite))
        })
    }

    pub fn revoke_invite(&self, id: &str, now: i64) -> Result<()> {
        self.with(Err(AccountError::Storage), |i| {
            let changed = i.conn.execute(
                "UPDATE invites SET revoked_at = ?2 WHERE id = ?1 AND used_at IS NULL AND revoked_at IS NULL",
                params![id, now],
            )?;
            Ok(if changed == 0 { Err(AccountError::Missing) } else { Ok(()) })
        })
    }

    pub fn accept_invite(
        &self,
        invite_hash: &str,
        name: &str,
        password_hash: &str,
        token_hash: &str,
        now: i64,
        expires: i64,
    ) -> Result<()> {
        self.with(Err(AccountError::Storage), |i| {
            let tx = i
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let Some(invite) = active_invite(&tx, invite_hash, now)? else {
                return Ok(Err(AccountError::Invalid));
            };
            let exists: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM users WHERE name = ?1)",
                [name],
                |r| r.get(0),
            )?;
            if exists {
                return Ok(Err(AccountError::Conflict));
            }
            if !valid_targets(&tx, invite.scopes.as_ref())? {
                return Ok(Err(AccountError::Invalid));
            }
            let user = insert_user(&tx, name, password_hash, invite.role, now)?;
            set_scopes(&tx, user, invite.scopes.as_ref(), now)?;
            insert_session(&tx, user, token_hash, now, expires)?;
            tx.execute(
                "UPDATE invites SET used_at = ?2 WHERE id = ?1",
                params![invite.id, now],
            )?;
            tx.commit()?;
            Ok(Ok(()))
        })
    }

    #[cfg(test)]
    pub fn create_session(&self, hash: &str, now: i64, expires: i64) -> bool {
        if !self.owner_exists() && self.recover_owner(None, "test", now).is_err() {
            return false;
        }
        let Some((id, stored, _)) = self.credential("admin") else {
            return false;
        };
        self.sign_in(id, &stored, hash, now, expires).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invites_expire_and_can_be_consumed_or_revoked_only_once() {
        let db = Db::open_in_memory().unwrap();
        db.recover_owner(None, "hash", 1).unwrap();
        for (hash, now) in [("expired", 9), ("revoked", 2), ("usable", 2)] {
            let invite = db.create_invite(hash, Role::Admin, None, 1, 9).unwrap();
            if hash == "revoked" {
                db.revoke_invite(&invite.id, now).unwrap();
                assert_eq!(
                    db.revoke_invite(&invite.id, now),
                    Err(AccountError::Missing)
                );
            }
            let active = hash == "usable";
            assert_eq!(db.validate_invite(hash, now).is_some(), active);
            let result = db.accept_invite(hash, "guest", "passhash", "session", now, 10);
            assert_eq!(
                result,
                if active {
                    Ok(())
                } else {
                    Err(AccountError::Invalid)
                }
            );
            assert_eq!(
                db.accept_invite(hash, "another", "passhash", "another-session", now, 10),
                Err(AccountError::Invalid)
            );
        }
    }

    #[test]
    fn frequent_logins_do_not_evict_other_users() {
        let db = Db::open_in_memory().unwrap();
        db.create_owner("owner", "owner-hash", "owner-session", 1, 100)
            .unwrap();
        db.create_invite("invite", Role::Viewer, None, 1, 100)
            .unwrap();
        db.accept_invite("invite", "guest", "guest-hash", "first-session", 1, 100)
            .unwrap();
        let (guest, _, _) = db.credential("guest").unwrap();
        for n in 0..MAX_SESSIONS {
            db.sign_in(
                guest,
                "guest-hash",
                &format!("guest-session-{n}"),
                2 + n,
                100,
            )
            .unwrap();
        }
        assert_eq!(db.session_count(30), (MAX_SESSIONS + 1) as u64);
        assert!(!db.session_is_valid("first-session", 30));
        assert!(db.session_is_valid("owner-session", 30));
    }

    #[test]
    fn password_reset_disable_and_deletion_revoke_only_the_target_sessions() {
        let db = Db::open_in_memory().unwrap();
        db.create_owner("owner", "owner-hash", "owner-session", 1, 10)
            .unwrap();
        db.create_invite("invite", Role::Viewer, None, 1, 10)
            .unwrap();
        db.accept_invite("invite", "guest", "guest-hash", "guest-session", 1, 10)
            .unwrap();
        let (guest, _, _) = db.credential("guest").unwrap();
        db.recover_owner(None, "new-owner-hash", 2).unwrap();
        assert!(!db.session_is_valid("owner-session", 2));
        assert!(db.session_is_valid("guest-session", 2));
        assert_eq!(
            db.sign_in(1, "owner-hash", "stale", 2, 10),
            Err(AccountError::Invalid)
        );
        db.change_account(guest, AccountChange::Disabled(true), 2)
            .unwrap();
        assert!(!db.session_is_valid("guest-session", 2));
        assert_eq!(
            db.sign_in(guest, "guest-hash", "disabled", 2, 10),
            Err(AccountError::Invalid)
        );
        db.change_account(guest, AccountChange::Disabled(false), 2)
            .unwrap();
        db.sign_in(guest, "guest-hash", "new-guest-session", 2, 10)
            .unwrap();
        db.change_account(guest, AccountChange::Delete, 3).unwrap();
        assert!(!db.session_is_valid("new-guest-session", 3));
    }
}
