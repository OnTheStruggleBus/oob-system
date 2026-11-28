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
/// digits: 13 address digits (the 40-bit address) plus 3 check digits.
///
/// **Status: documented** for the DCT700 (install manual p. B-10) and
/// DCT1700 (p. B-11) under "d 05: Unit Address" (oob-expert note on #21);
/// applying the split to the DCT2000 is same-family inference. The check
/// algorithm is unknown, so the check digits are kept verbatim: never
/// computed, never validated, never generated. Parse and display round-trip
/// losslessly.
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
            // Never echo the value: unit addresses are box-identifying
            // (ADR-003 §4, #25 QA-8).
            return Err(invalid(
                "unit address: want a 40-bit address and 3 check digits".into(),
            ));
        }
        Ok(Self { address, check })
    }

    /// The 40-bit address.
    pub fn address(self) -> u64 {
        self.address
    }

    /// The three check digits as printed (stored verbatim, not validated).
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
            return Err(invalid(UA_SHAPE.into()));
        }
        let digits = parts[..3].concat();
        let address = digits.parse().map_err(|_| invalid(UA_SHAPE.into()))?;
        let check = parts[3].parse().map_err(|_| invalid(UA_SHAPE.into()))?;
        Self::new(address, check)
    }
}

/// The unit-address shape, named in errors instead of the rejected value.
const UA_SHAPE: &str = "unit address: want nnn-nnnnn-nnnnn-nnn (16 digits)";

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
    /// Install date, `YYYY-MM-DD`.
    pub install_date: Option<String>,
    /// Auto-Registration Date (DSG), `YYYY-MM-DD`.
    pub auto_registration_date: Option<String>,
    /// Activated flag.
    pub activated: bool,
    /// Autodiscovered (DSG/DOCSIS).
    pub autodiscovered: bool,
    /// IP address (DSG/DOCSIS), IPv4 or IPv6 text form.
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

    /// Checks every field's domain (serial shape, dates, IP address).
    pub(crate) fn validate(&self) -> Result<(), StoreError> {
        if self.serial.len() != 12 || !self.serial.bytes().all(|b| b.is_ascii_alphanumeric()) {
            // Never echo the value: serials are box-identifying (QA-8).
            return Err(invalid("serial: want 12 ASCII alphanumerics".into()));
        }
        if self.box_type.is_empty() {
            return Err(invalid("empty box type".into()));
        }
        for (field, date) in [
            ("install_date", &self.install_date),
            ("auto_registration_date", &self.auto_registration_date),
        ] {
            if let Some(d) = date
                && !is_calendar_date(d)
            {
                return Err(invalid(format!("{field} {d:?} (want YYYY-MM-DD)")));
            }
        }
        if let Some(ip) = &self.ip_address
            && ip.parse::<std::net::IpAddr>().is_err()
        {
            return Err(invalid(format!("ip_address {ip:?}")));
        }
        Ok(())
    }
}

/// `YYYY-MM-DD` naming a real Gregorian calendar day.
pub(crate) fn is_calendar_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = &s[r];
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let (Some(y), Some(m), Some(d)) = (num(0..4), num(5..7), num(8..10)) else {
        return false;
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
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
    /// When the unit was retired (UTC, ISO 8601); `None` while in service.
    /// A retired unit is never deleted and its ID is never reused; it is
    /// excluded from targeting (#25).
    pub retired_utc: Option<String>,
}

impl Tx<'_> {
    /// Creates a terminal record and its `TerminalCreated` event in this
    /// transaction; returns the allocated ID.
    ///
    /// Crate-internal since #25: a unit is enrolled through
    /// `Command::Admin(AdminOp::UnitEnroll)`, which also writes the unit's
    /// first configuration revision and its journal row, so the terminal
    /// row and the latest revision never disagree.
    pub(crate) fn create_terminal(
        &self,
        actor: &str,
        t: &NewTerminal,
    ) -> Result<TerminalId, StoreError> {
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
    sacrificial, retired_utc FROM terminal";

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
        retired_utc: r.get(18)?,
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
