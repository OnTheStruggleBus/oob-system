//! Forward-only schema migrations on `PRAGMA user_version`.
//!
//! Each migration is an embedded SQL file numbered from 1. It runs in its
//! own transaction, which also bumps `user_version`, so a failed migration
//! leaves the file at the previous version. A file newer than
//! [`SCHEMA_VERSION`] is refused ([`StoreError::SchemaTooNew`]).

use rusqlite::Connection;

use super::StoreError;

/// `(number, sql)`, contiguous from 1. Released entries never change.
pub(crate) const MIGRATIONS: &[(u32, &str)] = &[
    (1, include_str!("migrations/0001_initial.sql")),
    (2, include_str!("migrations/0002_policy.sql")),
    (3, include_str!("migrations/0003_config.sql")),
];

/// The schema version this build migrates to (the head migration).
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// The file's current `user_version`.
pub(crate) fn user_version(conn: &Connection) -> Result<u32, StoreError> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// Brings `conn` to [`SCHEMA_VERSION`]; returns the number of migrations
/// applied (0 when already at head).
pub(crate) fn migrate(conn: &mut Connection) -> Result<u32, StoreError> {
    migrate_with(conn, MIGRATIONS)
}

/// [`migrate`] over an explicit migration list (tests inject a failing
/// migration through this, QA-6).
pub(crate) fn migrate_with(
    conn: &mut Connection,
    migrations: &[(u32, &str)],
) -> Result<u32, StoreError> {
    let head = migrations.last().map_or(0, |(n, _)| *n);
    let found = user_version(conn)?;
    if found > head {
        return Err(StoreError::SchemaTooNew {
            found,
            supported: head,
        });
    }
    let mut applied = 0;
    for &(to, sql) in migrations.iter().filter(|(n, _)| *n > found) {
        let step = |conn: &mut Connection| -> rusqlite::Result<()> {
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", to)?;
            tx.commit()
        };
        step(conn).map_err(|source| StoreError::Migrate { to, source })?;
        applied += 1;
    }
    Ok(applied)
}
