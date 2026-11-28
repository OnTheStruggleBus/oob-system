//! Configuration reads on a read-only store connection (#27's read model).
//! Reads are not commands; the REST adapter enforces who may read.

use crate::store::Reader;

use super::doc::{self, Doc};
use super::key::{ObjectKey, Revision, RevisionMeta};
use super::{ConfigError, HISTORY_PAGE_MAX, persist};

fn load(meta: RevisionMeta, json: &str) -> Result<Revision, ConfigError> {
    let kind = meta.key.kind;
    let doc: Doc = doc::upgrade(
        kind,
        meta.doc_schema,
        doc::doc_schema(kind),
        json,
        doc::UPGRADES,
    )?;
    Ok(Revision { meta, doc })
}

impl Reader {
    /// The head revision of `key`, if it has one.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] if the store fails or a document does not decode.
    pub fn config_head(&self, key: ObjectKey) -> Result<Option<Revision>, ConfigError> {
        persist::head(self.conn(), key)?
            .map(|(m, json)| load(m, &json))
            .transpose()
    }

    /// Revision `n` of `key`.
    ///
    /// # Errors
    ///
    /// [`ConfigError::UnknownRevision`] if there is none.
    pub fn config_revision(&self, key: ObjectKey, n: u32) -> Result<Revision, ConfigError> {
        let (m, json) = persist::revision(self.conn(), key, n)?
            .ok_or(ConfigError::UnknownRevision { key, revision: n })?;
        load(m, &json)
    }

    /// The history of `key`, oldest first: revisions after `after` (all
    /// when `None`), at most `limit`, clamped to `1..=`[`HISTORY_PAGE_MAX`].
    ///
    /// # Errors
    ///
    /// [`ConfigError`] if the store fails.
    pub fn config_history(
        &self,
        key: ObjectKey,
        after: Option<u32>,
        limit: u32,
    ) -> Result<Vec<RevisionMeta>, ConfigError> {
        Ok(persist::history(
            self.conn(),
            key,
            after.unwrap_or(0),
            limit.clamp(1, HISTORY_PAGE_MAX),
        )?)
    }
}
