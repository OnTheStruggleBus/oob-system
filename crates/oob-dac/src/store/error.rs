//! [`StoreError`]: every way the store can refuse or fail.

use std::fmt;

use rusqlite::ffi;

/// Which integrity rule a write broke.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintKind {
    /// A second terminal with an existing unit address.
    DuplicateUnitAddress,
    /// A second terminal with an existing serial number.
    DuplicateSerial,
    /// All 16 777 215 terminal IDs (24-bit, never reused) are spent.
    TerminalIdExhausted,
    /// A field outside its domain: a malformed unit address, serial, date or
    /// IP address, or a terminal type that is not one of the five Define
    /// Terminal types.
    Invalid(String),
    /// A schema `CHECK` failed; the text names the column or expression.
    Check(String),
    /// Any other `UNIQUE` / `PRIMARY KEY` collision.
    Unique(String),
    /// A row refers to a row that does not exist.
    ForeignKey,
}

/// Errors from [`Store`](super::Store) and its handle.
#[non_exhaustive]
#[derive(Debug)]
pub enum StoreError {
    /// The database file could not be opened or created.
    Open(std::io::Error),
    /// `PRAGMA quick_check` reported damage; the restore path is #24.
    Corrupt(String),
    /// The file's schema is newer than this build knows. An older Oob
    /// refuses a newer installation rather than corrupting it; there is no
    /// downgrade.
    SchemaTooNew {
        /// `PRAGMA user_version` found in the file.
        found: u32,
        /// The highest migration this build carries.
        supported: u32,
    },
    /// A migration failed; its transaction rolled back, so the file stays at
    /// `to - 1`.
    Migrate {
        /// The migration number that failed.
        to: u32,
        /// The underlying sqlite error.
        source: rusqlite::Error,
    },
    /// A write broke an integrity rule.
    Constraint(ConstraintKind),
    /// An `UPDATE` or `DELETE` was attempted on an append-only journal.
    AppendOnly {
        /// The journal table.
        table: &'static str,
    },
    /// An `event_log` payload did not decode (a bug, never operator input).
    Payload(serde_json::Error),
    /// The writer thread has stopped; the handle can no longer reach it.
    WriterGone,
    /// Any other sqlite error.
    Sqlite(rusqlite::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(e) => write!(f, "cannot open the store: {e}"),
            Self::Corrupt(s) => write!(f, "store failed its integrity check: {s}"),
            Self::SchemaTooNew { found, supported } => write!(
                f,
                "store schema version {found} is newer than this build supports ({supported}); \
                 refusing to open"
            ),
            Self::Migrate { to, source } => write!(f, "migration {to} failed: {source}"),
            Self::Constraint(k) => write!(f, "constraint violated: {k:?}"),
            Self::AppendOnly { table } => write!(f, "{table} is append-only"),
            Self::Payload(e) => write!(f, "event payload did not decode: {e}"),
            Self::WriterGone => write!(f, "the store writer thread has stopped"),
            Self::Sqlite(e) => write!(f, "sqlite: {e}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Open(e) => Some(e),
            Self::Migrate { source, .. } => Some(source),
            Self::Payload(e) => Some(e),
            Self::Sqlite(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        Self::Payload(e)
    }
}

/// The journals whose triggers raise `append-only: <table>`.
const APPEND_ONLY: &[&str] = &["event_log", "audit_log", "config_revision"];

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        let rusqlite::Error::SqliteFailure(ffi_err, msg) = &e else {
            return Self::Sqlite(e);
        };
        let msg = msg.clone().unwrap_or_default();
        match ffi_err.extended_code {
            ffi::SQLITE_CONSTRAINT_TRIGGER => {
                if let Some(rest) = msg.strip_prefix("append-only: ")
                    && let Some(table) = APPEND_ONLY.iter().find(|t| **t == rest)
                {
                    return Self::AppendOnly { table };
                }
                Self::Sqlite(e)
            }
            ffi::SQLITE_CONSTRAINT_UNIQUE | ffi::SQLITE_CONSTRAINT_PRIMARYKEY => {
                Self::Constraint(match msg.as_str() {
                    "UNIQUE constraint failed: terminal.unit_address" => {
                        ConstraintKind::DuplicateUnitAddress
                    }
                    "UNIQUE constraint failed: terminal.serial" => ConstraintKind::DuplicateSerial,
                    _ => ConstraintKind::Unique(msg),
                })
            }
            ffi::SQLITE_CONSTRAINT_CHECK => Self::Constraint(ConstraintKind::Check(msg)),
            ffi::SQLITE_CONSTRAINT_FOREIGNKEY => Self::Constraint(ConstraintKind::ForeignKey),
            _ => Self::Sqlite(e),
        }
    }
}
