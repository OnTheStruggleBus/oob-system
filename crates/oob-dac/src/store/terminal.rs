//! The Define Terminal record (Plan §3.2), Precious.
//!
//! `terminal_id` is allocated by the store and never supplied by a caller:
//! [`NewTerminal`] has no ID field, so no box instance can be seeded from git
//! (ADR-003 §6). Unit addresses and serials are box-identifying data; they
//! live only in the store file (ADR-003 §4).

use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ValueRef};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{ConstraintKind, Event, StoreError, Tx};

/// Highest 24-bit terminal ID.
pub const TERMINAL_ID_MAX: u32 = 16_777_215;

/// A store-allocated terminal ID, 1..=16 777 215 (24-bit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TerminalId(pub(crate) u32);

impl TerminalId {
    /// The raw ID.
    pub fn get(self) -> u32 {
        self.0
    }
}

/// The five Define Terminal types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalType {
    /// One-Way.
    OneWay,
    /// Fone-Way (telco return).
    FoneWay,
    /// Two-Way.
    TwoWay,
    /// Ethernet.
    Ethernet,
    /// DOCSIS.
    Docsis,
}

impl TerminalType {
    /// The Define Terminal label, as stored.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OneWay => "One-Way",
            Self::FoneWay => "Fone-Way",
            Self::TwoWay => "Two-Way",
            Self::Ethernet => "Ethernet",
            Self::Docsis => "DOCSIS",
        }
    }
}

impl FromStr for TerminalType {
    type Err = StoreError;
    fn from_str(s: &str) -> Result<Self, StoreError> {
        Ok(match s {
            "One-Way" => Self::OneWay,
            "Fone-Way" => Self::FoneWay,
            "Two-Way" => Self::TwoWay,
            "Ethernet" => Self::Ethernet,
            "DOCSIS" => Self::Docsis,
            other => {
                return Err(StoreError::Constraint(ConstraintKind::Invalid(format!(
                    "terminal type {other:?}"
                ))));
            }
        })
    }
}

/// A unit address as printed on the box: `nnn-nnnnn-nnnnn-nnn`, 16 decimal
/// digits. The first 13 are the 40-bit address; the last three are kept
/// verbatim (their check algorithm is not yet sourced, so they are stored,
/// not validated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnitAddress {
    address: u64,
    check: u16,
}

impl UnitAddress {
    /// Highest 40-bit address.
    pub const MAX: u64 = (1 << 40) - 1;

    /// Builds one from its parts; `address` must fit in 40 bits and
    /// `check` in three digits.
    pub fn new(address: u64, check: u16) -> Result<Self, StoreError> {
        if address > Self::MAX || check > 999 {
            return Err(invalid(format!("unit address {address}/{check}")));
        }
        Ok(Self { address, check })
    }

    /// The 40-bit address.
    pub fn address(self) -> u64 {
        self.address
    }

    /// The three trailing printed digits.
    pub fn check(self) -> u16 {
        self.check
    }
}

impl fmt::Display for UnitAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = format!("{:013}", self.address);
        write!(
            f,
            "{}-{}-{}-{:03}",
            &d[..3],
            &d[3..8],
            &d[8..13],
            self.check
        )
    }
}

impl FromStr for UnitAddress {
    type Err = StoreError;
    fn from_str(s: &str) -> Result<Self, StoreError> {
        let parts: Vec<&str> = s.split('-').collect();
        let shape_ok = parts.iter().map(|p| p.len()).eq([3, 5, 5, 3])
            && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()));
        if !shape_ok {
            return Err(invalid(format!("unit address {s:?}")));
        }
        let digits = parts[..3].concat();
        let address = digits
            .parse()
            .map_err(|_| invalid(format!("unit address {s:?}")))?;
        let check = parts[3]
            .parse()
            .map_err(|_| invalid(format!("unit address {s:?}")))?;
        Self::new(address, check)
    }
}

fn invalid(what: String) -> StoreError {
    StoreError::Constraint(ConstraintKind::Invalid(what))
}

/// The fields an operator enters for a new terminal (Define Terminal). No
/// terminal ID: the store allocates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTerminal {
    /// Unit address (burned into NVRAM, printed on the unit).
    pub unit_address: UnitAddress,
    /// Serial number, 12 ASCII alphanumerics (burned in).
    pub serial: String,
    /// One of the five Define Terminal types.
    pub terminal_type: TerminalType,
    /// Box type: an ID from the upstream catalog (ADR-003 QA-6, #30).
    pub box_type: String,
    /// Account number (issued by the business system).
    pub account_number: Option<String>,
    /// On-Plant flag.
    pub on_plant: bool,
    /// Install date (ISO 8601).
    pub install_date: Option<String>,
    /// Auto-Registration Date (DSG).
    pub auto_registration_date: Option<String>,
    /// Activated flag.
    pub activated: bool,
    /// Autodiscovered (DSG/DOCSIS).
    pub autodiscovered: bool,
    /// IP address (DSG/DOCSIS).
    pub ip_address: Option<String>,
    /// BSI name.
    pub bsi_name: Option<String>,
    /// Downstream plant.
    pub downstream_plant: Option<String>,
    /// Upstream plant.
    pub upstream_plant: Option<String>,
    /// The sacrificial unit for destructive operations (AGENTS.md §7; the
    /// lock itself is #23).
    pub sacrificial: bool,
}

impl NewTerminal {
    /// A record with the required fields set and everything else empty.
    pub fn new(
        unit_address: UnitAddress,
        serial: impl Into<String>,
        terminal_type: TerminalType,
        box_type: impl Into<String>,
    ) -> Self {
        Self {
            unit_address,
            serial: serial.into(),
            terminal_type,
            box_type: box_type.into(),
            account_number: None,
            on_plant: false,
            install_date: None,
            auto_registration_date: None,
            activated: false,
            autodiscovered: false,
            ip_address: None,
            bsi_name: None,
            downstream_plant: None,
            upstream_plant: None,
            sacrificial: false,
        }
    }

    fn validate(&self) -> Result<(), StoreError> {
        if self.serial.len() != 12 || !self.serial.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(invalid(format!("serial {:?}", self.serial)));
        }
        if self.box_type.is_empty() {
            return Err(invalid("empty box type".into()));
        }
        Ok(())
    }
}

/// A stored terminal record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terminal {
    /// The allocated ID.
    pub id: TerminalId,
    /// When the record was created (UTC, ISO 8601).
    pub created_utc: String,
    /// Everything the operator entered.
    pub record: NewTerminal,
}

impl Tx<'_> {
    /// Creates a terminal record and its `TerminalCreated` event in this
    /// transaction; returns the allocated ID.
    pub fn create_terminal(&self, actor: &str, t: &NewTerminal) -> Result<TerminalId, StoreError> {
        t.validate()?;
        let last: i64 = self
            .conn()
            .query_row(
                "SELECT seq FROM sqlite_sequence WHERE name = 'terminal'",
                [],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if last >= i64::from(TERMINAL_ID_MAX) {
            return Err(StoreError::Constraint(ConstraintKind::TerminalIdExhausted));
        }
        let id: u32 = self.conn().query_row(
            "INSERT INTO terminal (unit_address, ua_check, serial, terminal_type, box_type, \
             account_number, on_plant, install_date, auto_registration_date, activated, \
             autodiscovered, ip_address, bsi_name, downstream_plant, upstream_plant, sacrificial) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16) \
             RETURNING terminal_id",
            params![
                t.unit_address.address() as i64,
                t.unit_address.check(),
                t.serial,
                t.terminal_type.as_str(),
                t.box_type,
                t.account_number,
                t.on_plant,
                t.install_date,
                t.auto_registration_date,
                t.activated,
                t.autodiscovered,
                t.ip_address,
                t.bsi_name,
                t.downstream_plant,
                t.upstream_plant,
                t.sacrificial,
            ],
            |r| r.get(0),
        )?;
        self.append_event(actor, &Event::TerminalCreated { terminal_id: id })?;
        Ok(TerminalId(id))
    }

    /// Reads one terminal inside this transaction.
    pub fn terminal(&self, id: TerminalId) -> Result<Option<Terminal>, StoreError> {
        terminal(self.conn(), id)
    }
}

const SELECT: &str = "SELECT terminal_id, created_utc, unit_address, ua_check, serial, \
    terminal_type, box_type, account_number, on_plant, install_date, auto_registration_date, \
    activated, autodiscovered, ip_address, bsi_name, downstream_plant, upstream_plant, \
    sacrificial FROM terminal";

impl FromSql for TerminalType {
    fn column_result(v: ValueRef<'_>) -> FromSqlResult<Self> {
        v.as_str()?
            .parse()
            .map_err(|e: StoreError| FromSqlError::Other(e.to_string().into()))
    }
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<Terminal> {
    let address: i64 = r.get(2)?;
    Ok(Terminal {
        id: TerminalId(r.get(0)?),
        created_utc: r.get(1)?,
        record: NewTerminal {
            unit_address: UnitAddress {
                // The schema CHECK keeps it in 0..=2^40-1.
                address: address as u64,
                check: r.get(3)?,
            },
            serial: r.get(4)?,
            terminal_type: r.get(5)?,
            box_type: r.get(6)?,
            account_number: r.get(7)?,
            on_plant: r.get(8)?,
            install_date: r.get(9)?,
            auto_registration_date: r.get(10)?,
            activated: r.get(11)?,
            autodiscovered: r.get(12)?,
            ip_address: r.get(13)?,
            bsi_name: r.get(14)?,
            downstream_plant: r.get(15)?,
            upstream_plant: r.get(16)?,
            sacrificial: r.get(17)?,
        },
    })
}

pub(crate) fn terminal(conn: &Connection, id: TerminalId) -> Result<Option<Terminal>, StoreError> {
    Ok(conn
        .query_row(
            &format!("{SELECT} WHERE terminal_id = ?1"),
            [id.0],
            from_row,
        )
        .optional()?)
}

pub(crate) fn terminals(conn: &Connection) -> Result<Vec<Terminal>, StoreError> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY terminal_id"))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
