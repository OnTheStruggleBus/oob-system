//! OOB transport — TS writer, PSI/SI assemblers, PID plan,
//! insertion-bandwidth shaper.
//!
//! PSI/SI is DCII-flavored, not vanilla DVB (r5 F5-6, pending Marshal's
//! disposition). The stream is emitted asynchronous and null-stuffed at
//! the SCTE 55-1 Mode A aggregate of 2.048 Mb/s (QPSK, 1.024 Msym/s —
//! ADR-001), with a ~1000 Kbps insertion budget for DAC traffic. The GNU
//! Radio flowgraph owns RF timing; this crate owns bytes. Slice C (W2)
//! lands the writer; this crate currently establishes the workspace
//! boundary.

// Intentionally empty: Slice C (W2) seeds the TS writer here.
