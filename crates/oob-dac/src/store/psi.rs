//! PSI table versions (#17 hand-off to #25), Precious.
//!
//! oob-mux keeps, per PSI table kind, the 5-bit `version_number` it last
//! emitted and the section bytes at that version, so a changed table gets
//! the next version. Without persistence a restart re-emits a changed table
//! at version 0, which a box that cached that version may ignore. The daemon
//! (#29) converts oob-mux's snapshot into [`PsiTableVersion`] rows and back;
//! the store holds raw values only, so no oob-mux type crosses into it. The
//! daemon writes the kind as [`psi_kind`] defines it. Wiring the save on
//! emit and the restore at start is #29's.
//!
//! **Pruning policy (owned by #25): none.** [`Tx::save_psi_table_versions`]
//! upserts and never deletes, so a table that left the plan keeps its
//! version (`retired`) and a re-add continues the mod-32 count. The set is
//! bounded by two plus the number of program numbers ever used (one PMT
//! per program), so it stays small. Dropping a retired entry early would reintroduce the
//! restart-at-version-0 hazard; an explicit owner reset is the only future
//! way to drop one, and none exists yet.

use rusqlite::{Connection, params};

use super::{ConstraintKind, StoreError, Tx};

/// One table kind's persisted version state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PsiTableVersion {
    /// The table kind, encoded as [`psi_kind`] defines: `pat`, `cat`, or
    /// `pmt:<program_number>` (1..=65535, decimal, no leading zero).
    pub kind: String,
    /// The 5-bit `version_number`, 0..=31.
    pub version: u8,
    /// The section bytes last emitted at that version.
    pub bytes: Vec<u8>,
    /// The table left the plan; its version is kept.
    pub retired: bool,
}

/// Whether `kind` is a valid encoded PSI table kind (A-9 on #87): `pat`,
/// `cat`, or `pmt:<program_number>` with a program number in 1..=65535
/// written in decimal without a leading zero. A PMT is per program, so the
/// set is bounded by two plus the program numbers ever used.
pub fn psi_kind(kind: &str) -> bool {
    match kind {
        "pat" | "cat" => true,
        k => k.strip_prefix("pmt:").is_some_and(|n| {
            !n.is_empty()
                && !n.starts_with('0')
                && n.bytes().all(|b| b.is_ascii_digit())
                && n.parse::<u16>().is_ok_and(|v| v >= 1)
        }),
    }
}

impl Tx<'_> {
    /// Upserts every row in this transaction. Rows not named are kept (the
    /// pruning policy is "never", see the module docs).
    ///
    /// # Errors
    ///
    /// [`StoreError::Constraint`] for a kind [`psi_kind`] refuses or a
    /// version above 31; nothing is written then.
    pub fn save_psi_table_versions(&self, rows: &[PsiTableVersion]) -> Result<(), StoreError> {
        for r in rows {
            if !psi_kind(&r.kind) || r.version > 31 {
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
