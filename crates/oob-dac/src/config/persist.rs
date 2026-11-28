//! SQL for the configuration store: `config_revision` rows and the unit
//! columns of `terminal`. Only [`super::ConfigService`] writes through it.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::store::{StoreError, Terminal, TerminalId};

use super::check::UnitView;
use super::doc::UnitDoc;
use super::key::{ConfigKind, ObjectKey, Origin, RevisionMeta};

const COLS: &str = "kind, object_id, revision, doc_sha256, doc_schema, origin, rolled_back_to, \
                    actor, ts_utc, audit_seq";

fn corrupt(what: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(StoreError::Corrupt(what)),
    )
}

fn meta_from_row(r: &Row<'_>) -> rusqlite::Result<RevisionMeta> {
    let kind_text: String = r.get(0)?;
    let kind = ConfigKind::parse(&kind_text)
        .ok_or_else(|| corrupt(format!("config_revision kind {kind_text:?}")))?;
    let object_text: String = r.get(1)?;
    let object_id = if object_text.is_empty() {
        None
    } else {
        Some(TerminalId(object_text.parse().map_err(|_| {
            corrupt(format!("config_revision object_id {object_text:?}"))
        })?))
    };
    let origin_text: String = r.get(5)?;
    Ok(RevisionMeta {
        key: ObjectKey { kind, object_id },
        revision: r.get(2)?,
        doc_sha256: r.get(3)?,
        doc_schema: r.get(4)?,
        origin: Origin::parse(&origin_text)
            .ok_or_else(|| corrupt(format!("config_revision origin {origin_text:?}")))?,
        rolled_back_to: r.get(6)?,
        actor: r.get(7)?,
        ts_utc: r.get(8)?,
        audit_seq: r.get(9)?,
    })
}

/// The head (highest revision) of `key`, with its stored document text.
pub(crate) fn head(
    conn: &Connection,
    key: ObjectKey,
) -> Result<Option<(RevisionMeta, String)>, StoreError> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {COLS}, doc FROM config_revision WHERE kind = ?1 AND object_id = ?2 \
                 ORDER BY revision DESC LIMIT 1"
            ),
            params![key.kind.as_str(), key.object_text()],
            |r| Ok((meta_from_row(r)?, r.get(10)?)),
        )
        .optional()?)
}

/// Revision `n` of `key`, with its stored document text.
pub(crate) fn revision(
    conn: &Connection,
    key: ObjectKey,
    n: u32,
) -> Result<Option<(RevisionMeta, String)>, StoreError> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {COLS}, doc FROM config_revision WHERE kind = ?1 AND object_id = ?2 \
                 AND revision = ?3"
            ),
            params![key.kind.as_str(), key.object_text(), n],
            |r| Ok((meta_from_row(r)?, r.get(10)?)),
        )
        .optional()?)
}

/// History of `key`, oldest first, after revision `after`, at most `limit`.
pub(crate) fn history(
    conn: &Connection,
    key: ObjectKey,
    after: u32,
    limit: u32,
) -> Result<Vec<RevisionMeta>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM config_revision WHERE kind = ?1 AND object_id = ?2 AND revision > ?3 \
         ORDER BY revision LIMIT ?4"
    ))?;
    let rows = stmt.query_map(
        params![key.kind.as_str(), key.object_text(), after, limit],
        meta_from_row,
    )?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// How many revisions objects of `kind` have.
pub(crate) fn count(conn: &Connection, kind: ConfigKind) -> Result<i64, StoreError> {
    Ok(conn.query_row(
        "SELECT count(*) FROM config_revision WHERE kind = ?1",
        [kind.as_str()],
        |r| r.get(0),
    )?)
}

/// One `config_revision` row to insert.
pub(crate) struct NewRevision<'a> {
    pub(crate) key: ObjectKey,
    pub(crate) revision: u32,
    pub(crate) doc: &'a str,
    pub(crate) sha: &'a str,
    pub(crate) schema: u32,
    pub(crate) origin: Origin,
    pub(crate) rolled_back_to: Option<u32>,
    pub(crate) actor: &'a str,
    pub(crate) audit_seq: i64,
}

pub(crate) fn insert(conn: &Connection, r: &NewRevision<'_>) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO config_revision (kind, object_id, revision, doc, doc_sha256, doc_schema, \
         origin, rolled_back_to, actor, audit_seq) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            r.key.kind.as_str(),
            r.key.object_text(),
            r.revision,
            r.doc,
            r.sha,
            r.schema,
            r.origin.as_str(),
            r.rolled_back_to,
            r.actor,
            r.audit_seq,
        ],
    )?;
    Ok(())
}

/// Writes a unit's operational record into its `terminal` row. Identity
/// columns are never in this statement.
pub(crate) fn write_unit_row(
    conn: &Connection,
    id: TerminalId,
    d: &UnitDoc,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE terminal SET terminal_type = ?1, box_type = ?2, account_number = ?3, \
         on_plant = ?4, install_date = ?5, activated = ?6, autodiscovered = ?7, \
         ip_address = ?8, bsi_name = ?9, downstream_plant = ?10, upstream_plant = ?11, \
         sacrificial = ?12 WHERE terminal_id = ?13",
        params![
            d.terminal_type,
            d.box_type,
            d.account_number,
            d.on_plant,
            d.install_date,
            d.activated,
            d.autodiscovered,
            d.ip_address,
            d.bsi_name,
            d.downstream_plant,
            d.upstream_plant,
            d.sacrificial,
            id.get(),
        ],
    )?;
    if n != 1 {
        return Err(StoreError::Corrupt(format!(
            "terminal {} vanished during a unit write",
            id.get()
        )));
    }
    Ok(())
}

/// Sets or clears `retired_utc`.
pub(crate) fn set_retired(
    conn: &Connection,
    id: TerminalId,
    retired: bool,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE terminal SET retired_utc = CASE WHEN ?1 \
         THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE NULL END WHERE terminal_id = ?2",
        params![retired, id.get()],
    )?;
    Ok(())
}

/// The candidate unit set: every terminal row's operational record.
#[derive(Debug, Clone, Default)]
pub(crate) struct Units {
    pub(crate) map: BTreeMap<TerminalId, (UnitDoc, bool)>,
}

impl Units {
    pub(crate) fn load(conn: &Connection) -> Result<Self, StoreError> {
        let map = crate::store::terminals_of(conn)?
            .into_iter()
            .map(|t: Terminal| {
                let retired = t.retired_utc.is_some();
                (t.id, (UnitDoc::from_record(&t.record), retired))
            })
            .collect();
        Ok(Self { map })
    }
}

impl UnitView for Units {
    fn ids(&self) -> Vec<TerminalId> {
        self.map.keys().copied().collect()
    }
    fn get(&self, id: TerminalId) -> Option<&UnitDoc> {
        self.map.get(&id).map(|(d, _)| d)
    }
    fn is_retired(&self, id: TerminalId) -> bool {
        self.map.get(&id).is_some_and(|(_, r)| *r)
    }
}
