//! The audit log (the bsilog equivalent, DAC Reference §8): Precious and
//! append-only. Schema triggers reject every `UPDATE` and `DELETE`
//! ([`StoreError::AppendOnly`]). One audit path, shared by the command layer
//! (#23, F6-10) and the configuration journal (#25).

use rusqlite::{Connection, params};

use super::{StoreError, Tx};

/// One audit record to append.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// Who acted.
    pub actor: String,
    /// The role they acted in.
    pub role: String,
    /// The front door the command came through (API, Console, CLI, …).
    pub front_door: String,
    /// The command or verb.
    pub command: String,
    /// What it targeted, if anything.
    pub target: Option<String>,
    /// How it ended.
    pub outcome: String,
    /// State before, as JSON.
    pub before: Option<String>,
    /// State after, as JSON.
    pub after: Option<String>,
}

/// A stored audit record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    /// Sequence number (monotonic, never reused).
    pub seq: i64,
    /// When it was recorded (UTC, ISO 8601).
    pub ts_utc: String,
    /// What was recorded.
    pub entry: AuditEntry,
}

impl Tx<'_> {
    /// Appends an audit record in this transaction; returns its sequence
    /// number.
    pub fn append_audit(&self, e: &AuditEntry) -> Result<i64, StoreError> {
        Ok(self.conn().query_row(
            "INSERT INTO audit_log (actor, role, front_door, command, target, outcome, before, \
             after) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING seq",
            params![
                e.actor,
                e.role,
                e.front_door,
                e.command,
                e.target,
                e.outcome,
                e.before,
                e.after
            ],
            |r| r.get(0),
        )?)
    }
}

pub(crate) fn audit_records(conn: &Connection) -> Result<Vec<AuditRecord>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT seq, ts_utc, actor, role, front_door, command, target, outcome, before, after \
         FROM audit_log ORDER BY seq",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(AuditRecord {
            seq: r.get(0)?,
            ts_utc: r.get(1)?,
            entry: AuditEntry {
                actor: r.get(2)?,
                role: r.get(3)?,
                front_door: r.get(4)?,
                command: r.get(5)?,
                target: r.get(6)?,
                outcome: r.get(7)?,
                before: r.get(8)?,
                after: r.get(9)?,
            },
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
