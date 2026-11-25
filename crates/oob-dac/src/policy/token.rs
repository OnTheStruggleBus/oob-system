//! Confirmation tokens (r6 F6-10, #23 design note section 3).
//!
//! A token authorizes **one** destructive operation on **one** target. It is
//! issued by a separate call ([`StorePolicy::issue_token`]) after the caller
//! types the confirmation phrase, is 128 random bits shown once (only its
//! SHA-256 is stored), expires after a short TTL, and is spent by the first
//! check that uses it.
//!
//! [`StorePolicy::issue_token`]: super::StorePolicy::issue_token

use sha2::{Digest, Sha256};

use crate::command::target::TerminalRef;
use crate::store::{StoreError, TerminalId, Tx};

use super::RuleId;
use super::classes::DestructiveOp;

/// What a token is bound to besides the operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenTarget {
    /// One terminal (its store-allocated ID; serials never leave the store,
    /// ADR-003 section 4).
    Terminal(TerminalId),
    /// A resolved bulk target set, by [`plan_digest`].
    Plan(String),
    /// This installation, by its install ID.
    Install(String),
}

impl TokenTarget {
    /// The stored binding, such as `terminal:12`.
    pub(crate) fn key(&self) -> String {
        match self {
            Self::Terminal(id) => format!("terminal:{}", id.get()),
            Self::Plan(d) => format!("plan:{d}"),
            Self::Install(i) => format!("install:{i}"),
        }
    }

    /// The short label typed after the phrase.
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Terminal(id) => id.get().to_string(),
            Self::Plan(d) => d.chars().take(12).collect(),
            Self::Install(i) => i.clone(),
        }
    }

    pub(crate) fn kind(&self) -> super::classes::TargetKind {
        use super::classes::TargetKind as K;
        match self {
            Self::Terminal(_) => K::Terminal,
            Self::Plan(_) => K::Plan,
            Self::Install(_) => K::Install,
        }
    }
}

/// The phrase a caller must type to get a token: the class phrase and the
/// target label, such as `FACTORY INIT 12` or `CDL LOCK OFF 0123456789abcdef`.
/// The server compares it; a script cannot skip the typed confirmation of
/// AGENTS.md section 7.
pub fn confirmation_phrase(op: DestructiveOp, target: &TokenTarget) -> String {
    format!("{} {}", op.row().phrase, target.label())
}

/// SHA-256 hex of the operation name and the sorted terminal IDs of a
/// resolved set. A bulk token is bound to this digest, so a set that changed
/// between the dry run and the execute is refused (`token-plan-changed`).
pub fn plan_digest(operation: &str, resolved: &[TerminalRef]) -> String {
    let mut ids: Vec<u32> = resolved.iter().map(|t| t.id.get()).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut h = Sha256::new();
    h.update(b"oob-plan-v1\0");
    h.update(operation.as_bytes());
    for id in ids {
        h.update(b"\0");
        h.update(id.to_be_bytes());
    }
    hex(&h.finalize())
}

pub(crate) fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Spends the token if it authorizes exactly (`op`, `target`) now. Returns
/// the refusing rule, or `None` when the token was valid and is now used.
/// Runs in the caller's transaction, and the writer is single, so of two
/// racing spends exactly one wins. A refused attempt does not spend the
/// token.
pub(crate) fn consume(
    tx: &Tx<'_>,
    token: &str,
    op: DestructiveOp,
    target: &TokenTarget,
    now_ms: i64,
) -> Result<Option<RuleId>, StoreError> {
    use rusqlite::OptionalExtension;
    let hash = token_hash(token);
    let row: Option<(String, String, i64, i64)> = tx
        .conn()
        .query_row(
            "SELECT operation, target, expires_ms, used FROM confirmation_token \
             WHERE token_hash = ?1",
            [&hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((operation, bound, expires_ms, used)) = row else {
        return Ok(Some(RuleId::TokenUnknown));
    };
    if used != 0 {
        return Ok(Some(RuleId::TokenReused));
    }
    if now_ms >= expires_ms {
        return Ok(Some(RuleId::TokenExpired));
    }
    if operation != op.name() {
        return Ok(Some(RuleId::TokenOpMismatch));
    }
    let want = target.key();
    if bound != want {
        return Ok(Some(
            if bound.starts_with("plan:") && want.starts_with("plan:") {
                RuleId::TokenPlanChanged
            } else {
                RuleId::TokenTargetMismatch
            },
        ));
    }
    let spent = tx.conn().execute(
        "UPDATE confirmation_token SET used = 1 WHERE token_hash = ?1 AND used = 0",
        [&hash],
    )?;
    Ok((spent != 1).then_some(RuleId::TokenReused))
}
