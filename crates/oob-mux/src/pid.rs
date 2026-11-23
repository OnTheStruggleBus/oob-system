//! Packet identifiers.
//!
//! A [`Pid`] is a 13-bit transport packet identifier. The null PID,
//! 0x1FFF, is deliberately *not* representable: null packets are built
//! only inside the pacer (ADR-002 decision 2, r6 F6-2), so no PDU and no
//! packetizer output can ever carry it. The 13-bit field and the 0x1FFF
//! null value are generic MPEG-2 transport structure; for the OOB stream
//! that framing is the r4 lead and stays `hypothesized` until a capture
//! verifies it.

use core::fmt;

/// The highest value a 13-bit PID field can hold.
const PID_MAX: u16 = 0x1FFF;

/// The null PID. Reserved for the pacer's stuffing packets; never a valid
/// [`Pid`].
pub(crate) const NULL_PID_VALUE: u16 = 0x1FFF;

/// A transport packet identifier that is never the null PID.
///
/// ```
/// use oob_mux::{Pid, PidError};
///
/// assert_eq!(Pid::new(0x0000).map(Pid::value), Ok(0x0000));
/// assert_eq!(Pid::new(0x1FFF), Err(PidError::Reserved));
/// assert_eq!(Pid::new(0x2000), Err(PidError::OutOfRange(0x2000)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Pid(u16);

/// Why a value is not a usable [`Pid`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PidError {
    /// The value does not fit the 13-bit PID field.
    OutOfRange(u16),
    /// 0x1FFF is the null PID, which only the pacer may emit.
    Reserved,
}

impl Pid {
    /// Builds a PID, rejecting values above 13 bits and the null PID.
    pub const fn new(value: u16) -> Result<Self, PidError> {
        if value > PID_MAX {
            Err(PidError::OutOfRange(value))
        } else if value == NULL_PID_VALUE {
            Err(PidError::Reserved)
        } else {
            Ok(Self(value))
        }
    }

    /// The raw 13-bit value.
    pub const fn value(self) -> u16 {
        self.0
    }
}

impl fmt::Display for Pid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:04X}", self.0)
    }
}

impl fmt::Display for PidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange(v) => write!(f, "PID 0x{v:04X} does not fit 13 bits"),
            Self::Reserved => f.write_str("PID 0x1FFF is the null PID, reserved to the pacer"),
        }
    }
}

impl std::error::Error for PidError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_pid_is_rejected() {
        assert_eq!(Pid::new(0x1FFF), Err(PidError::Reserved));
    }

    #[test]
    fn range_is_thirteen_bits() {
        assert_eq!(Pid::new(0x1FFE).map(Pid::value), Ok(0x1FFE));
        assert_eq!(Pid::new(0x2000), Err(PidError::OutOfRange(0x2000)));
        assert_eq!(Pid::new(u16::MAX), Err(PidError::OutOfRange(u16::MAX)));
    }
}
