//! OOB transport — TS writer, PSI/SI assemblers, PID plan,
//! insertion-bandwidth shaper.
//!
//! DCII-flavored PSI/SI, not vanilla DVB (F5-6), emitted as an
//! asynchronous, null-stuffed stream at 1.024 Msym/s (~1.544 Mb/s) with a
//! ~1000 Kbps insertion budget for DAC traffic. The GNU Radio flowgraph
//! owns RF timing; this crate owns bytes. Slice C (W2) lands the writer;
//! this crate currently establishes the workspace boundary.

// Intentionally empty: Slice C (W2) seeds the TS writer here.
