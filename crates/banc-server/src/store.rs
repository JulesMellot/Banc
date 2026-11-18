//! Reads and writes of the server database.
//!
//! Every function takes the current time as a parameter (Unix seconds), so
//! that tests decide what time it is. Functions that change several rows run
//! in a transaction.

use std::str::FromStr;

use banc_proto::keys::PublicKey;
use banc_proto::machine::{Inventory, MachineRole, Os, PersonRole, ResolveInstall};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

/// Lifetime of an invitation (section 2 of the plan).
pub const INVITE_LIFETIME_SECS: i64 = 7 * 24 * 3600;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("SQLite error")]
    Sqlite(#[from] rusqlite::Error),
    #[error("JSON error")]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

/// Who did something, for the audit log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Server,
    Admin(i64),
    Machine(i64),
}

impl Actor {
    fn label(self) -> String {
        match self {
            Self::Server => "server".to_owned(),
            Self::Admin(id) => format!("admin:{id}"),
            Self::Machine(id) => format!("machine:{id}"),
        }
    }
}

/// Writes a line to the audit log.
pub fn log_event(
    conn: &Connection,
    now: i64,
    actor: Actor,
    kind: &str,
    machine_id: Option<i64>,
    detail: &serde_json::Value,
) -> Result<()> {
    conn.execute(
        "INSERT INTO events (at, actor, kind, machine_id, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![now, actor.label(), kind, machine_id, detail.to_string()],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- admins

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Admin {
    pub id: i64,
    pub name: String,
}

pub fn admin_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT count(*) FROM admins", [], |row| row.get(0))?)
}

/// Creates an admin. `None` if the name is taken.
pub fn create_admin(
    conn: &Connection,
    name: &str,
    password_hash: &str,
    now: i64,
) -> Result<Option<i64>> {
    let inserted = conn.execute(
        "INSERT INTO admins (name, password_hash, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (name) DO NOTHING",
        params![name, password_hash, now],
    )?;
    if inserted == 0 {
        return Ok(None);
    }
    let id = conn.last_insert_rowid();
    log_event(
        conn,
        now,
        Actor::Admin(id),
        "admin_created",
        None,
        &serde_json::json!({ "name": name }),
    )?;
    Ok(Some(id))
}

/// The admin and their password hash.
pub fn admin_by_name(conn: &Connection, name: &str) -> Result<Option<(Admin, String)>> {
    Ok(conn
        .query_row(
            "SELECT id, name, password_hash FROM admins WHERE name = ?1",
            [name],
            |row| {
                Ok((
                    Admin {
                        id: row.get(0)?,
                        name: row.get(1)?,
                    },
                    row.get(2)?,
                ))
            },
        )
        .optional()?)
}

pub fn create_session(
    conn: &Connection,
    token_hash: &[u8; 32],
    admin_id: i64,
    now: i64,
    expires_at: i64,
) -> Result<()> {
    // Expired sessions are cleaned up here rather than by a timer: no
    // background activity on the server either.
    conn.execute("DELETE FROM admin_sessions WHERE expires_at <= ?1", [now])?;
    conn.execute(
        "INSERT INTO admin_sessions (token_hash, admin_id, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![token_hash, admin_id, now, expires_at],
    )?;
    Ok(())
}

/// The admin of a session still valid at `now`.
pub fn session_admin(conn: &Connection, token_hash: &[u8; 32], now: i64) -> Result<Option<Admin>> {
    Ok(conn
        .query_row(
            "SELECT admins.id, admins.name FROM admin_sessions
             JOIN admins ON admins.id = admin_sessions.admin_id
             WHERE token_hash = ?1 AND expires_at > ?2",
            params![token_hash, now],
            |row| {
                Ok(Admin {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            },
        )
        .optional()?)
}

pub fn delete_session(conn: &Connection, token_hash: &[u8; 32]) -> Result<()> {
    conn.execute(
        "DELETE FROM admin_sessions WHERE token_hash = ?1",
        [token_hash],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- public address

/// The public address learned from the first admin's request.
pub fn public_url(conn: &Connection) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT public_url FROM server_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()?)
}

pub fn set_public_url(conn: &Connection, url: &str, now: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO server_settings (id, public_url, updated_at) VALUES (1, ?1, ?2)
         ON CONFLICT (id) DO UPDATE SET public_url = ?1, updated_at = ?2",
        params![url, now],
    )?;
    log_event(
        conn,
        now,
        Actor::Server,
        "public_url_learned",
        None,
        &serde_json::json!({ "public_url": url }),
    )
}

// ---------------------------------------------------------------- admin key

pub fn admin_key(conn: &Connection) -> Result<Option<PublicKey>> {
    let bytes: Option<Vec<u8>> = conn
        .query_row(
            "SELECT admin_public_key FROM team WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(bytes.as_deref().and_then(public_key))
}

/// Registers the team's admin key. Pending invitations made for another key
/// stop working: their link carries the old fingerprint.
pub fn set_admin_key(
    conn: &mut Connection,
    key: &PublicKey,
    admin_id: i64,
    now: i64,
) -> Result<()> {
    let tx = conn.transaction()?;
    let previous = admin_key(&tx)?;
    tx.execute(
        "INSERT INTO team (id, admin_public_key, updated_at) VALUES (1, ?1, ?2)
         ON CONFLICT (id) DO UPDATE SET admin_public_key = ?1, updated_at = ?2",
        params![key.as_bytes(), now],
    )?;
    let detail = serde_json::json!({
        "public_key": key.to_string(),
        "previous": previous.map(|key| key.to_string()),
    });
    log_event(
        &tx,
        now,
        Actor::Admin(admin_id),
        "admin_key_set",
        None,
        &detail,
    )?;
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------- invitations

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteStatus {
    Pending,
    Used,
    Expired,
    Revoked,
    /// Made for an admin key that has since been replaced.
    Obsolete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InviteRow {
    pub id: i64,
    pub user_name: String,
    pub person_role: PersonRole,
    pub machine_role: MachineRole,
    pub created_at: i64,
    pub expires_at: i64,
    pub used_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub machine_id: Option<i64>,
    pub status: InviteStatus,
}

/// Status of an invitation at `now`. A used or revoked invitation keeps that
/// status after its expiry date.
pub fn invite_status(
    used_at: Option<i64>,
    revoked_at: Option<i64>,
    expires_at: i64,
    made_for_current_key: bool,
    now: i64,
) -> InviteStatus {
    if used_at.is_some() {
        InviteStatus::Used
    } else if revoked_at.is_some() {
        InviteStatus::Revoked
    } else if now >= expires_at {
        InviteStatus::Expired
    } else if !made_for_current_key {
        InviteStatus::Obsolete
    } else {
        InviteStatus::Pending
    }
}

pub struct NewInvite<'a> {
    pub token_hash: &'a [u8; 32],
    pub user_name: &'a str,
    pub person_role: PersonRole,
    pub machine_role: MachineRole,
    pub admin_key: &'a PublicKey,
    pub created_by: i64,
}

pub fn create_invite(conn: &Connection, invite: &NewInvite<'_>, now: i64) -> Result<InviteRow> {
    let expires_at = now + INVITE_LIFETIME_SECS;
    conn.execute(
        "INSERT INTO invites (token_hash, user_name, person_role, machine_role,
                              admin_public_key, created_by, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            invite.token_hash,
            invite.user_name,
            invite.person_role.as_str(),
            invite.machine_role.as_str(),
            invite.admin_key.as_bytes(),
            invite.created_by,
            now,
            expires_at
        ],
    )?;
    let id = conn.last_insert_rowid();
    let detail = serde_json::json!({
        "invite_id": id,
        "user_name": invite.user_name,
        "person_role": invite.person_role,
        "machine_role": invite.machine_role,
    });
    log_event(
        conn,
        now,
        Actor::Admin(invite.created_by),
        "invite_created",
        None,
        &detail,
    )?;
    Ok(InviteRow {
        id,
        user_name: invite.user_name.to_owned(),
        person_role: invite.person_role,
        machine_role: invite.machine_role,
        created_at: now,
        expires_at,
        used_at: None,
        revoked_at: None,
        machine_id: None,
        status: InviteStatus::Pending,
    })
}

/// All invitations, most recent first.
pub fn list_invites(conn: &Connection, now: i64) -> Result<Vec<InviteRow>> {
    let current = admin_key(conn)?;
    let mut statement = conn.prepare(
        "SELECT id, user_name, person_role, machine_role, created_at, expires_at,
                used_at, revoked_at, machine_id, admin_public_key
         FROM invites ORDER BY created_at DESC, id DESC",
    )?;
    let rows = statement.query_map([], |row| {
        let used_at = row.get(6)?;
        let revoked_at = row.get(7)?;
        let expires_at = row.get(5)?;
        let key: Vec<u8> = row.get(9)?;
        let made_for_current_key = current.is_some_and(|current| current.as_bytes()[..] == key[..]);
        Ok(InviteRow {
            id: row.get(0)?,
            user_name: row.get(1)?,
            person_role: parse(row, 2)?,
            machine_role: parse(row, 3)?,
            created_at: row.get(4)?,
            expires_at,
            used_at,
            revoked_at,
            machine_id: row.get(8)?,
            status: invite_status(used_at, revoked_at, expires_at, made_for_current_key, now),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Revokes a pending invitation. `false` if it does not exist or can no
/// longer be used anyway.
pub fn revoke_invite(conn: &Connection, id: i64, admin_id: i64, now: i64) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE invites SET revoked_at = ?2
         WHERE id = ?1 AND used_at IS NULL AND revoked_at IS NULL",
        params![id, now],
    )?;
    if changed > 0 {
        let detail = serde_json::json!({ "invite_id": id });
        log_event(
            conn,
            now,
            Actor::Admin(admin_id),
            "invite_revoked",
            None,
            &detail,
        )?;
    }
    Ok(changed > 0)
}

// ---------------------------------------------------------------- enrollment

/// Why an enrollment is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EnrollRefusal {
    #[error("unknown invitation")]
    UnknownInvite,
    #[error("invitation already used")]
    InviteUsed,
    #[error("invitation revoked")]
    InviteRevoked,
    #[error("invitation expired")]
    InviteExpired,
    #[error("the admin key changed since the invitation was made, ask for a new one")]
    AdminKeyChanged,
    #[error("this machine key is already enrolled")]
    KeyAlreadyEnrolled,
}

#[derive(Debug, thiserror::Error)]
pub enum EnrollError {
    #[error(transparent)]
    Refused(#[from] EnrollRefusal),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for EnrollError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(error.into())
    }
}

/// What the machine declares when it enrolls.
pub struct NewMachine<'a> {
    pub public_key: &'a PublicKey,
    pub name: &'a str,
    pub os: Os,
    pub arch: &'a str,
    pub agent_version: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enrolled {
    pub machine_id: i64,
    pub machine_role: MachineRole,
    pub person_role: PersonRole,
    pub admin_key: PublicKey,
}

/// Consumes an invitation and records the machine, atomically: a token can
/// only ever produce one machine.
pub fn enroll(
    conn: &mut Connection,
    token_hash: &[u8; 32],
    machine: &NewMachine<'_>,
    now: i64,
) -> Result<Enrolled, EnrollError> {
    let tx = conn.transaction()?;
    let invite = tx
        .query_row(
            "SELECT id, user_name, person_role, machine_role, admin_public_key,
                    expires_at, used_at, revoked_at
             FROM invites WHERE token_hash = ?1",
            [token_hash],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    parse::<PersonRole>(row, 2)?,
                    parse::<MachineRole>(row, 3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                ))
            },
        )
        .optional()?;
    let Some((
        invite_id,
        user_name,
        person_role,
        machine_role,
        invite_key,
        expires_at,
        used_at,
        revoked_at,
    )) = invite
    else {
        return Err(EnrollRefusal::UnknownInvite.into());
    };
    let current_key = admin_key(&tx)?;
    let made_for_current_key =
        current_key.is_some_and(|current| current.as_bytes()[..] == invite_key[..]);
    match invite_status(used_at, revoked_at, expires_at, made_for_current_key, now) {
        InviteStatus::Pending => {}
        InviteStatus::Used => return Err(EnrollRefusal::InviteUsed.into()),
        InviteStatus::Revoked => return Err(EnrollRefusal::InviteRevoked.into()),
        InviteStatus::Expired => return Err(EnrollRefusal::InviteExpired.into()),
        InviteStatus::Obsolete => return Err(EnrollRefusal::AdminKeyChanged.into()),
    }
    let Some(admin_key) = current_key else {
        return Err(EnrollRefusal::AdminKeyChanged.into());
    };

    // A revoked key stays refused: a machine that comes back enrolls with a
    // new key.
    let key_known: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM machines WHERE public_key = ?1)",
        [machine.public_key.as_bytes()],
        |row| row.get(0),
    )?;
    if key_known {
        return Err(EnrollRefusal::KeyAlreadyEnrolled.into());
    }

    tx.execute(
        "INSERT INTO machines (name, public_key, os, arch, role, user_name, person_role,
                               enrolled_at, agent_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            machine.name,
            machine.public_key.as_bytes(),
            machine.os.as_str(),
            machine.arch,
            machine_role.as_str(),
            user_name,
            person_role.as_str(),
            now,
            machine.agent_version
        ],
    )?;
    let machine_id = tx.last_insert_rowid();
    tx.execute(
        "UPDATE invites SET used_at = ?2, machine_id = ?3 WHERE id = ?1",
        params![invite_id, now, machine_id],
    )?;
    let detail = serde_json::json!({
        "invite_id": invite_id,
        "name": machine.name,
        "os": machine.os,
        "arch": machine.arch,
        "public_key": machine.public_key.to_string(),
    });
    log_event(
        &tx,
        now,
        Actor::Machine(machine_id),
        "machine_enrolled",
        Some(machine_id),
        &detail,
    )?;
    tx.commit()?;
    Ok(Enrolled {
        machine_id,
        machine_role,
        person_role,
        admin_key,
    })
}

// ---------------------------------------------------------------- machines

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MachineRow {
    pub id: i64,
    pub name: String,
    pub public_key: String,
    pub os: Os,
    pub arch: String,
    pub role: MachineRole,
    pub user_name: String,
    pub person_role: PersonRole,
    pub enrolled_at: i64,
    pub last_seen_at: Option<i64>,
    pub agent_version: String,
    pub os_version: Option<String>,
    pub hostname: Option<String>,
    pub resolve: Option<ResolveInstall>,
    pub revoked_at: Option<i64>,
}

const MACHINE_COLUMNS: &str = "id, name, public_key, os, arch, role, user_name, person_role,
    enrolled_at, last_seen_at, agent_version, os_version, hostname, resolve_edition,
    resolve_version, revoked_at";

fn machine_row(row: &Row<'_>) -> rusqlite::Result<MachineRow> {
    let key: Vec<u8> = row.get(2)?;
    let edition: Option<String> = row.get(13)?;
    let version: Option<String> = row.get(14)?;
    let resolve = match (edition, version) {
        (Some(edition), Some(version)) => Some(ResolveInstall {
            edition: edition.parse().map_err(|e| conversion(13, e))?,
            version,
        }),
        _ => None,
    };
    Ok(MachineRow {
        id: row.get(0)?,
        name: row.get(1)?,
        public_key: public_key(&key).map(|k| k.to_string()).unwrap_or_default(),
        os: parse(row, 3)?,
        arch: row.get(4)?,
        role: parse(row, 5)?,
        user_name: row.get(6)?,
        person_role: parse(row, 7)?,
        enrolled_at: row.get(8)?,
        last_seen_at: row.get(9)?,
        agent_version: row.get(10)?,
        os_version: row.get(11)?,
        hostname: row.get(12)?,
        resolve,
        revoked_at: row.get(15)?,
    })
}

/// All machines, revoked ones included, by name.
pub fn list_machines(conn: &Connection) -> Result<Vec<MachineRow>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {MACHINE_COLUMNS} FROM machines ORDER BY name COLLATE NOCASE, id"
    ))?;
    let rows = statement.query_map([], machine_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The non-revoked machine with this key.
pub fn active_machine(conn: &Connection, key: &PublicKey) -> Result<Option<MachineRow>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {MACHINE_COLUMNS} FROM machines
                 WHERE public_key = ?1 AND revoked_at IS NULL"
            ),
            [key.as_bytes()],
            machine_row,
        )
        .optional()?)
}

/// The agent connected: records its version and inventory. Logs an event
/// when the Resolve version changes.
pub fn record_hello(
    conn: &mut Connection,
    machine_id: i64,
    agent_version: &str,
    inventory: &Inventory,
    now: i64,
) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE machines SET agent_version = ?2, last_seen_at = ?3 WHERE id = ?1",
        params![machine_id, agent_version, now],
    )?;
    record_inventory(&tx, machine_id, inventory, now)?;
    tx.commit()?;
    Ok(())
}

/// A new inventory from a connected agent.
pub fn record_report(
    conn: &mut Connection,
    machine_id: i64,
    inventory: &Inventory,
    now: i64,
) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE machines SET last_seen_at = ?2 WHERE id = ?1",
        params![machine_id, now],
    )?;
    record_inventory(&tx, machine_id, inventory, now)?;
    tx.commit()?;
    Ok(())
}

fn record_inventory(
    conn: &Connection,
    machine_id: i64,
    inventory: &Inventory,
    now: i64,
) -> Result<()> {
    let previous: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT resolve_edition, resolve_version FROM machines WHERE id = ?1",
            [machine_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let edition = inventory.resolve.as_ref().map(|r| r.edition.as_str());
    let version = inventory.resolve.as_ref().map(|r| r.version.as_str());
    conn.execute(
        "UPDATE machines SET os_version = ?2, hostname = ?3, resolve_edition = ?4,
                             resolve_version = ?5, last_report = ?6
         WHERE id = ?1",
        params![
            machine_id,
            inventory.os_version,
            inventory.hostname,
            edition,
            version,
            serde_json::to_string(inventory)?
        ],
    )?;
    if let Some((old_edition, old_version)) = previous {
        if old_edition.as_deref() != edition || old_version.as_deref() != version {
            let detail = serde_json::json!({
                "from": old_version,
                "to": version,
                "edition": edition,
            });
            log_event(
                conn,
                now,
                Actor::Machine(machine_id),
                "resolve_changed",
                Some(machine_id),
                &detail,
            )?;
        }
    }
    Ok(())
}

/// Heartbeat: the machine is alive.
pub fn touch_machine(conn: &Connection, machine_id: i64, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE machines SET last_seen_at = ?2 WHERE id = ?1",
        params![machine_id, now],
    )?;
    Ok(())
}

/// Revokes a machine. Returns its key, to cut its connections, or `None` if
/// it does not exist or is already revoked.
pub fn revoke_machine(
    conn: &Connection,
    machine_id: i64,
    admin_id: i64,
    now: i64,
) -> Result<Option<PublicKey>> {
    let key: Option<Vec<u8>> = conn
        .query_row(
            "UPDATE machines SET revoked_at = ?2
             WHERE id = ?1 AND revoked_at IS NULL
             RETURNING public_key",
            params![machine_id, now],
            |row| row.get(0),
        )
        .optional()?;
    let Some(key) = key else {
        return Ok(None);
    };
    log_event(
        conn,
        now,
        Actor::Admin(admin_id),
        "machine_revoked",
        Some(machine_id),
        &serde_json::json!({}),
    )?;
    Ok(public_key(&key))
}

// ---------------------------------------------------------------- helpers

fn public_key(bytes: &[u8]) -> Option<PublicKey> {
    let bytes: [u8; 32] = bytes.try_into().ok()?;
    PublicKey::from_bytes(&bytes).ok()
}

fn parse<T>(row: &Row<'_>, index: usize) -> rusqlite::Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let text: String = row.get(index)?;
    text.parse().map_err(|e| conversion(index, e))
}

fn conversion<E>(index: usize, error: E) -> rusqlite::Error
where
    E: std::error::Error + Send + Sync + 'static,
{
    rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(error))
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
