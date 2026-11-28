//! The upstream box-type catalog (ADR-003 §2, Marshal's QA-6 ruling:
//! "Boxes of known types are runtime; a new box type is added only through
//! an upstream PR or a fork").
//!
//! [`BoxTypeCatalog`] is **read-only**: it answers whether a box type
//! exists and nothing else. There is no method, API, command or document
//! kind that adds, edits or removes a box type at runtime; a unit whose box
//! type is not in the catalog the install shipped with is refused
//! (`unknown-box-type`). A test reads this file and fails on any method that
//! takes a mutable receiver.
//!
//! The production catalog is the `box_types` section of the shipped seed
//! data (#30), read once at start ([`FileCatalog::load`]); its digest is
//! logged and goes into export manifests (#24). Until #30 settles the file
//! format the loader reads JSON, `{"box_types": [{"id": "dct2000", …}]}`;
//! fields other than `id` (the profile itself) belong to #30 and are
//! ignored here.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use serde::Deserialize;

use super::doc::sha256_hex;

/// The read-only catalog of box types this install knows.
pub trait BoxTypeCatalog: Send + Sync {
    /// Whether `box_type` is in the catalog.
    fn contains(&self, box_type: &str) -> bool;
    /// Every box type ID, sorted.
    fn ids(&self) -> Vec<String>;
    /// SHA-256 hex identifying this catalog's contents.
    fn digest(&self) -> String;
}

/// A catalog from a fixed list, for tests and doctests. The daemon loads
/// the catalog the install shipped with ([`FileCatalog`]).
#[derive(Debug, Clone)]
pub struct InMemoryCatalog {
    ids: BTreeSet<String>,
}

impl InMemoryCatalog {
    /// A catalog of exactly `ids`.
    pub fn new<I, S>(ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            ids: ids.into_iter().map(Into::into).collect(),
        }
    }
}

fn digest_of(ids: &BTreeSet<String>) -> String {
    let joined: Vec<&str> = ids.iter().map(String::as_str).collect();
    sha256_hex(joined.join("\n").as_bytes())
}

impl BoxTypeCatalog for InMemoryCatalog {
    fn contains(&self, box_type: &str) -> bool {
        self.ids.contains(box_type)
    }
    fn ids(&self) -> Vec<String> {
        self.ids.iter().cloned().collect()
    }
    fn digest(&self) -> String {
        digest_of(&self.ids)
    }
}

/// Why a catalog file was refused.
#[derive(Debug)]
#[non_exhaustive]
pub enum CatalogError {
    /// The file could not be read.
    Io(std::io::Error),
    /// The file is not a catalog document.
    Parse(String),
    /// An entry has an empty ID.
    EmptyId,
    /// Two entries share an ID.
    DuplicateId(String),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read the box-type catalog: {e}"),
            Self::Parse(e) => write!(f, "the box-type catalog does not parse: {e}"),
            Self::EmptyId => f.write_str("a box-type catalog entry has an empty id"),
            Self::DuplicateId(id) => write!(f, "box type {id:?} is listed twice"),
        }
    }
}

impl std::error::Error for CatalogError {}

#[derive(Deserialize)]
struct CatalogFile {
    box_types: Vec<CatalogEntry>,
}

#[derive(Deserialize)]
struct CatalogEntry {
    id: String,
}

/// The shipped catalog, read once from a file.
#[derive(Debug, Clone)]
pub struct FileCatalog {
    ids: BTreeSet<String>,
    digest: String,
}

impl FileCatalog {
    /// Parses a catalog document. The digest covers the file's bytes.
    ///
    /// # Errors
    ///
    /// [`CatalogError`].
    pub fn from_json(bytes: &[u8]) -> Result<Self, CatalogError> {
        let file: CatalogFile =
            serde_json::from_slice(bytes).map_err(|e| CatalogError::Parse(e.to_string()))?;
        let mut ids = BTreeSet::new();
        for e in file.box_types {
            if e.id.is_empty() {
                return Err(CatalogError::EmptyId);
            }
            if !ids.insert(e.id.clone()) {
                return Err(CatalogError::DuplicateId(e.id));
            }
        }
        Ok(Self {
            ids,
            digest: sha256_hex(bytes),
        })
    }

    /// Reads and parses the catalog at `path`.
    ///
    /// # Errors
    ///
    /// [`CatalogError`].
    pub fn load(path: &Path) -> Result<Self, CatalogError> {
        Self::from_json(&std::fs::read(path).map_err(CatalogError::Io)?)
    }
}

impl BoxTypeCatalog for FileCatalog {
    fn contains(&self, box_type: &str) -> bool {
        self.ids.contains(box_type)
    }
    fn ids(&self) -> Vec<String> {
        self.ids.iter().cloned().collect()
    }
    fn digest(&self) -> String {
        self.digest.clone()
    }
}
