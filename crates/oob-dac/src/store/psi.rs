//! PSI table versions (#17 hand-off to #25), Precious.
//!
//! oob-mux keeps, per PSI table kind, the 5-bit `version_number` it last
//! emitted and the section bytes at that version, so a changed table gets
//! the next version. Without persistence a restart re-emits a changed table
//! at version 0, which a box that cached that version may ignore. The daemon
//! (#29) converts oob-mux's snapshot into [`PsiTableVersion`] rows and back;
//! the store holds raw values only, so no oob-mux type crosses into it.
//!
//! **Pruning policy (owned by #25): none.** [`Tx::save_psi_table_versions`]
//! upserts and never deletes, so a table that left the plan keeps its
//! version (`retired`) and a re-add continues the mod-32 count. The set is
//! bounded by the number of PSI table kinds, so it cannot grow without
//! limit. Dropping a retired entry early would reintroduce the
//! restart-at-version-0 hazard; an explicit owner reset is the only future
//! way to drop one, and none exists yet.

use rusqlite::{Connection, params};

use super::{ConstraintKind, StoreError, Tx};

/// One table kind's persisted version state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PsiTableVersion {
    /// The table kind's name as the daemon writes it (for example `Pat`).
    pub kind: String,
    /// The 5-bit `version_number`, 0..=31.
    pub version: u8,
    /// The section bytes last emitted at that version.
    pub bytes: Vec<u8>,
    /// The table left the plan; its version is kept.
    pub retired: bool,
}

impl Tx<'_> {
    /// Upserts every row in this transaction. Rows not named are kept (the
    /// pruning policy is "never", see the module docs).
    ///
    /// # Errors
    ///
    /// [`StoreError::Constraint`] for an empty kind or a version above 31;
    /// nothing is written then.
    pub fn save_psi_table_versions(&self, rows: &[PsiTableVersion]) -> Result<(), StoreError> {
        for r in rows {
            if r.kind.is_empty() || r.version > 31 {
                return Err(StoreError::Constraint(ConstraintKind::Invalid(format!(
                    "psi table version {:?}/{}",
                    r.kind, r.version
                ))));
            }
        }
        let mut stmt = self.conn().prepare(
            "INSERT INTO psi_table_version (kind, version, bytes, retired) VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT (kind) DO UPDATE SET version = excluded.version, bytes = excluded.bytes, \
             retired = excluded.retired, \
             updated_utc = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )?;
        for r in rows {
            stmt.execute(params![r.kind, r.version, r.bytes, r.retired])?;
        }
        Ok(())
    }
}

pub(crate) fn psi_table_versions(conn: &Connection) -> Result<Vec<PsiTableVersion>, StoreError> {
    let mut stmt =
        conn.prepare("SELECT kind, version, bytes, retired FROM psi_table_version ORDER BY kind")?;
    let rows = stmt.query_map([], |r| {
        Ok(PsiTableVersion {
            kind: r.get(0)?,
            version: r.get(1)?,
            bytes: r.get(2)?,
            retired: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
