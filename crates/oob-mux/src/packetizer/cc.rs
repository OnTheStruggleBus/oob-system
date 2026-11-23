//! Per-PID continuity counters, owned by the packetizer.

use crate::pid::Pid;

/// One 4-bit counter per PID, allocated once.
pub(crate) struct ContinuityCounters(Box<[u8; 8192]>);

impl ContinuityCounters {
    pub(crate) fn new() -> Self {
        Self(Box::new([0; 8192]))
    }

    /// The counter for this packet on `pid`; advances it mod 16.
    pub(crate) fn next(&mut self, pid: Pid) -> u8 {
        let c = &mut self.0[usize::from(pid.value())];
        let v = *c;
        *c = (v + 1) & 0x0F;
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_per_pid() {
        let a = Pid::new(0x10).unwrap_or_else(|e| panic!("{e}"));
        let b = Pid::new(0x11).unwrap_or_else(|e| panic!("{e}"));
        let mut cc = ContinuityCounters::new();
        let seq: Vec<u8> = (0..17).map(|_| cc.next(a)).collect();
        assert_eq!(seq[..16], (0..16).collect::<Vec<u8>>()[..]);
        assert_eq!(seq[16], 0);
        assert_eq!(cc.next(b), 0);
        assert_eq!(cc.next(a), 1);
    }
}
