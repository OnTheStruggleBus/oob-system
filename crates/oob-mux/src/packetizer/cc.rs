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

    proptest::proptest! {
        /// F6-15: across any interleaving of PIDs, each PID's counter is
        /// +1 mod 16 per packet, from 0, wrapping at 16; other PIDs never
        /// disturb it. (The null PID never reaches here: `Pid` cannot hold
        /// 0x1FFF, so null packets are exempt by construction.)
        #[test]
        fn counters_are_independent_and_wrap(
            picks in proptest::collection::vec(0usize..5, 0..400)
        ) {
            const PIDS: [u16; 5] = [0x0000, 0x0001, 0x0100, 0x1D00, 0x1FFE];
            let mut cc = ContinuityCounters::new();
            let mut count = [0usize; 5];
            for i in picks {
                let pid = Pid::new(PIDS[i]).unwrap_or_else(|e| panic!("{e}"));
                proptest::prop_assert_eq!(usize::from(cc.next(pid)), count[i] % 16);
                count[i] += 1;
            }
        }
    }
}
