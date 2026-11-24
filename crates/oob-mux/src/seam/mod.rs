//! The pull seam, mux side: the endpoint the flowgraph pulls from (ADR-002
//! decision 2, r6 F6-2, issue #15). The wire contract is on the mdBook
//! page `protocol/seam.md` and in [`proto`]; this module serves it.
//!
//! The GNU Radio flowgraph is the clock master. A custom source block
//! sends a 24-byte request over an `AF_UNIX` stream socket; [`serve`]
//! calls [`PullSource::pull`] for exactly that many packets and answers
//! with a 16-byte header and the packets. One request is outstanding at a
//! time, so the channel is bounded by construction. The seam's unit is the
//! 188-byte transport packet; SCTE 55-1 coding stays in the flowgraph.
//!
//! # Pull semantics
//!
//! - **Exactly n.** The reply always holds the `n` packets asked for; the
//!   source null-fills any shortfall (null packets exist only in the
//!   pacer, so the endpoint never makes one).
//! - **No puller, no airtime.** While no flowgraph is connected the source
//!   is not pulled, so stream time stands still. The flowgraph covers its
//!   own absences with nulls and counts them (`puller_underruns`,
//!   `puller_nulls`), which the endpoint records but never replays.
//! - **One puller.** A second connection while one is live is accepted
//!   only to be closed and counted, because two pullers would drain the
//!   pacer twice. A successor that connects in the same accept batch as
//!   a departing puller's close is counted refused and simply retries
//!   (the flowgraph reconnects about once a second). A request left
//!   half-sent for more than two seconds drops the connection, so a wedged
//!   peer cannot hold the slot.
//! - **Refusal.** A malformed or over-limit request, or a source that
//!   refuses the pull, gets a header with `status = 1` and the connection
//!   is closed; the flowgraph then treats the endpoint as absent.
//!
//! The data path is one blocking thread (no tokio) over `std` sockets.
//!
//! # Logging
//!
//! Rare events (refused connections, bad requests, disconnects) go to
//! stderr with an `oob-mux seam:` prefix until the daemon's logging (#29)
//! exists; the counters in [`SeamStats`] are the durable record.

pub mod proto;
pub mod stub;

use std::fmt;
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::pacer::{PacerConfig, PullSource};
use crate::packet::{PACKET_LEN, TsPacket};
use proto::{
    PACKET_SIZE_V1, REQUEST_LEN, RESPONSE_HEADER_LEN, Request, ResponseHeader, STATUS_BAD_REQUEST,
    STATUS_OK,
};

/// How often the serve loop looks up from a quiet socket to check for a
/// second connection and for shutdown.
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// How long a reply may take to write before the connection is dropped.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a half-sent request may stay half-sent before the connection
/// is dropped, so a wedged peer cannot hold the single-puller slot.
const PARTIAL_FRAME_TIMEOUT: Duration = Duration::from_secs(2);

/// Seam endpoint settings. The socket path and `max_pull` are Layer 1
/// install settings (ADR-003); the frame constants are code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeamConfig {
    /// Where to listen; `/run/oob/seam.sock` by default in a deployment.
    pub socket_path: PathBuf,
    /// The largest `n` served. There is one `max_pull`: take it from the
    /// pacer ([`SeamConfig::from_pacer`]) so the two cannot drift.
    pub max_pull: u16,
    /// Bytes per packet; 188 in v1 (`hypothesized`).
    pub packet_size: u16,
}

impl SeamConfig {
    /// Settings for serving `pacer`: its `max_pull`, packet size 188.
    ///
    /// # Errors
    ///
    /// [`SeamError::MaxPullTooLarge`] when the pacer's `max_pull` does not
    /// fit the protocol's 16-bit `n`.
    pub fn from_pacer(
        socket_path: impl Into<PathBuf>,
        pacer: &PacerConfig,
    ) -> Result<Self, SeamError> {
        let max_pull = u16::try_from(pacer.max_pull).map_err(|_| SeamError::MaxPullTooLarge {
            max_pull: pacer.max_pull,
        })?;
        Ok(Self {
            socket_path: socket_path.into(),
            max_pull,
            packet_size: PACKET_SIZE_V1,
        })
    }

    fn validate(&self) -> Result<(), SeamError> {
        if self.max_pull == 0 {
            return Err(SeamError::BadConfig("max_pull is zero"));
        }
        if self.packet_size != PACKET_SIZE_V1 {
            return Err(SeamError::BadConfig("packet_size is not 188 (v1)"));
        }
        Ok(())
    }
}

/// Why [`serve`] stopped or could not start.
#[derive(Debug)]
#[non_exhaustive]
pub enum SeamError {
    /// The socket could not be bound (including another live endpoint
    /// already listening on the path).
    Bind(io::Error),
    /// A listener-level I/O failure. Per-connection errors never end
    /// [`serve`].
    Io(io::Error),
    /// The pacer's `max_pull` exceeds what the protocol's `n` can carry.
    MaxPullTooLarge {
        /// The pacer's value.
        max_pull: usize,
    },
    /// The settings are unusable.
    BadConfig(&'static str),
}

impl fmt::Display for SeamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bind(e) => write!(f, "seam socket bind failed: {e}"),
            Self::Io(e) => write!(f, "seam listener failed: {e}"),
            Self::MaxPullTooLarge { max_pull } => {
                write!(f, "max_pull {max_pull} exceeds the protocol's 16-bit n")
            }
            Self::BadConfig(why) => write!(f, "seam configuration refused: {why}"),
        }
    }
}

impl std::error::Error for SeamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Bind(e) | Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

/// Asks [`serve`] to return. Shared between the serving thread and
/// whoever stops it.
#[derive(Debug, Default)]
pub struct ShutdownFlag(AtomicBool);

impl ShutdownFlag {
    /// A flag that is not set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests shutdown.
    pub fn request(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Whether shutdown was requested.
    pub fn is_requested(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// The endpoint's counters: plain atomics, read by `/metrics` (#29) and by
/// the loopback harness. The puller's own counters are the last cumulative
/// values it reported, not sums.
#[derive(Debug, Default)]
pub struct SeamStats {
    /// Requests answered `OK`.
    pub requests: AtomicU64,
    /// Packets sent in `OK` replies.
    pub packets: AtomicU64,
    /// The flowgraph's last reported count of requests it null-filled.
    pub puller_underruns: AtomicU64,
    /// The flowgraph's last reported count of packets it null-filled.
    pub puller_nulls: AtomicU64,
    /// Sum of `pacer_nulls` over `OK` replies: normal pacer fill.
    pub pacer_nulls: AtomicU64,
    /// Requests refused with `status = 1`.
    pub bad_requests: AtomicU64,
    /// Connections accepted as the puller.
    pub connections: AtomicU64,
    /// Second connections closed because a puller was live.
    pub refused_connections: AtomicU64,
}

impl SeamStats {
    /// A consistent-enough copy for logging; each field is read once.
    pub fn snapshot(&self) -> SeamStatsSnapshot {
        let get = |a: &AtomicU64| a.load(Ordering::Relaxed);
        SeamStatsSnapshot {
            requests: get(&self.requests),
            packets: get(&self.packets),
            puller_underruns: get(&self.puller_underruns),
            puller_nulls: get(&self.puller_nulls),
            pacer_nulls: get(&self.pacer_nulls),
            bad_requests: get(&self.bad_requests),
            connections: get(&self.connections),
            refused_connections: get(&self.refused_connections),
        }
    }
}

/// A plain copy of [`SeamStats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeamStatsSnapshot {
    /// See [`SeamStats::requests`].
    pub requests: u64,
    /// See [`SeamStats::packets`].
    pub packets: u64,
    /// See [`SeamStats::puller_underruns`].
    pub puller_underruns: u64,
    /// See [`SeamStats::puller_nulls`].
    pub puller_nulls: u64,
    /// See [`SeamStats::pacer_nulls`].
    pub pacer_nulls: u64,
    /// See [`SeamStats::bad_requests`].
    pub bad_requests: u64,
    /// See [`SeamStats::connections`].
    pub connections: u64,
    /// See [`SeamStats::refused_connections`].
    pub refused_connections: u64,
}

impl SeamStatsSnapshot {
    /// The F6-2 null ratio as the endpoint sees it: (pacer nulls + the
    /// puller's own nulls) over all packets emitted toward the modulator
    /// (served + puller-filled). The puller's counters lag by one request.
    /// `0.0` before any packet.
    pub fn null_ratio(&self) -> f64 {
        let total = self.packets + self.puller_nulls;
        if total == 0 {
            0.0
        } else {
            (self.pacer_nulls + self.puller_nulls) as f64 / total as f64
        }
    }

    /// One plain-text line for the G0.5 evidence log; the flowgraph's own
    /// log carries the same terms from its side.
    pub fn log_line(&self) -> String {
        format!(
            "requests={} packets={} puller_underruns={} puller_nulls={} pacer_nulls={} \
             bad_requests={} connections={} refused={} null_ratio={:.4}",
            self.requests,
            self.packets,
            self.puller_underruns,
            self.puller_nulls,
            self.pacer_nulls,
            self.bad_requests,
            self.connections,
            self.refused_connections,
            self.null_ratio()
        )
    }
}

/// Binds `path`, replacing a stale socket file left by a crashed endpoint
/// but never one a live endpoint is listening on.
fn bind(path: &Path) -> Result<UnixListener, SeamError> {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        // Only ever unlink a socket: on Linux, connecting to a regular
        // file also gives ECONNREFUSED, so a misconfigured path must not
        // be mistaken for a stale socket (oob-qa F-1).
        if !meta.file_type().is_socket() {
            return Err(SeamError::Bind(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "the seam socket path exists and is not a socket; refusing to remove it",
            )));
        }
        match UnixStream::connect(path) {
            Ok(_) => {
                return Err(SeamError::Bind(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another endpoint is listening on the seam socket",
                )));
            }
            Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                std::fs::remove_file(path).map_err(SeamError::Bind)?;
            }
            Err(e) => return Err(SeamError::Bind(e)),
        }
    }
    let listener = UnixListener::bind(path).map_err(SeamError::Bind)?;
    // Access is the directory's and the group's: 0660 (design note §2).
    // There is a brief window between bind and chmod in which the umask's
    // mode applies; the socket directory's permissions are what close it
    // (ops, ADR-002 decision 6).
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))
        .map_err(SeamError::Bind)?;
    listener.set_nonblocking(true).map_err(SeamError::Io)?;
    Ok(listener)
}

/// What one look at the connection found.
enum Step {
    /// A request was answered.
    Served,
    /// Nothing arrived within the poll interval.
    Idle,
    /// The connection is over (EOF, error or refusal); drop it.
    Closed,
}

/// The live connection and its partially read request.
struct Conn {
    stream: UnixStream,
    req: [u8; REQUEST_LEN],
    filled: usize,
    /// When the first byte of the current partial request arrived.
    partial_since: Option<Instant>,
}

/// Serves the seam on `cfg.socket_path` until `shutdown` is requested.
/// Run it on a dedicated OS thread: it blocks.
///
/// A per-connection error drops that connection and goes back to
/// accepting; only an unusable setting or a bind failure ends the call
/// early. On return the socket file is removed.
///
/// # Errors
///
/// [`SeamError::BadConfig`] for unusable settings, [`SeamError::Bind`]
/// when the socket cannot be bound.
pub fn serve<S: PullSource>(
    cfg: &SeamConfig,
    mut source: S,
    stats: Arc<SeamStats>,
    shutdown: &ShutdownFlag,
) -> Result<(), SeamError> {
    cfg.validate()?;
    let listener = bind(&cfg.socket_path)?;
    let mut out: Vec<TsPacket> = Vec::with_capacity(usize::from(cfg.max_pull));
    let mut buf: Vec<u8> =
        Vec::with_capacity(RESPONSE_HEADER_LEN + usize::from(cfg.max_pull) * PACKET_LEN);
    let mut conn: Option<Conn> = None;

    while !shutdown.is_requested() {
        accept_pending(&listener, &mut conn, &stats);
        match conn.as_mut() {
            None => std::thread::sleep(POLL_INTERVAL),
            Some(c) => match step(c, cfg, &mut source, &stats, &mut out, &mut buf) {
                Step::Served | Step::Idle => {}
                Step::Closed => {
                    eprintln!("oob-mux seam: puller disconnected");
                    conn = None;
                }
            },
        }
    }
    drop(conn);
    // Best effort: a leftover file is replaced at the next start anyway.
    let _ = std::fs::remove_file(&cfg.socket_path);
    Ok(())
}

/// Takes every pending connection: the first becomes the puller when none
/// is live, every other is closed and counted.
fn accept_pending(listener: &UnixListener, conn: &mut Option<Conn>, stats: &SeamStats) {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if conn.is_some() {
                    stats.refused_connections.fetch_add(1, Ordering::Relaxed);
                    eprintln!("oob-mux seam: second connection refused (a puller is live)");
                    drop(stream);
                    continue;
                }
                let ready = stream.set_nonblocking(false).is_ok()
                    && stream.set_read_timeout(Some(POLL_INTERVAL)).is_ok()
                    && stream.set_write_timeout(Some(WRITE_TIMEOUT)).is_ok();
                if ready {
                    stats.connections.fetch_add(1, Ordering::Relaxed);
                    eprintln!("oob-mux seam: puller connected");
                    *conn = Some(Conn {
                        stream,
                        req: [0; REQUEST_LEN],
                        filled: 0,
                        partial_since: None,
                    });
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                eprintln!("oob-mux seam: accept failed: {e}");
                return;
            }
        }
    }
}

/// Reads toward one request and, when whole, answers it.
fn step<S: PullSource>(
    c: &mut Conn,
    cfg: &SeamConfig,
    source: &mut S,
    stats: &SeamStats,
    out: &mut Vec<TsPacket>,
    buf: &mut Vec<u8>,
) -> Step {
    if c.partial_since
        .is_some_and(|t| t.elapsed() > PARTIAL_FRAME_TIMEOUT)
    {
        eprintln!("oob-mux seam: request half-sent for too long, dropping the puller");
        return Step::Closed;
    }
    match c.stream.read(&mut c.req[c.filled..]) {
        Ok(0) => return Step::Closed,
        Ok(n) => c.filled += n,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) =>
        {
            return Step::Idle;
        }
        Err(e) if e.kind() == io::ErrorKind::Interrupted => return Step::Idle,
        Err(e) => {
            eprintln!("oob-mux seam: read failed: {e}");
            return Step::Closed;
        }
    }
    if c.filled < REQUEST_LEN {
        c.partial_since.get_or_insert_with(Instant::now);
        return Step::Idle;
    }
    c.filled = 0;
    c.partial_since = None;
    answer(c, cfg, source, stats, out, buf)
}

/// Answers one whole request frame.
fn answer<S: PullSource>(
    c: &mut Conn,
    cfg: &SeamConfig,
    source: &mut S,
    stats: &SeamStats,
    out: &mut Vec<TsPacket>,
    buf: &mut Vec<u8>,
) -> Step {
    let req = match Request::decode(&c.req)
        .and_then(|r| r.check(cfg.max_pull, cfg.packet_size).map(|()| r))
    {
        Ok(r) => r,
        Err(e) => {
            // The seq is only trustworthy when the frame was ours.
            let seq = Request::decode(&c.req).map_or(0, |r| r.seq);
            return refuse(c, stats, seq, &format!("bad request: {e}"));
        }
    };
    stats
        .puller_underruns
        .store(u64::from(req.puller_underruns), Ordering::Relaxed);
    stats
        .puller_nulls
        .store(u64::from(req.puller_nulls), Ordering::Relaxed);

    out.clear();
    let report = match source.pull(usize::from(req.n), out) {
        Ok(r) => r,
        Err(e) => return refuse(c, stats, req.seq, &format!("source refused the pull: {e}")),
    };
    if out.len() != usize::from(req.n) || report.nulls > out.len() {
        return refuse(c, stats, req.seq, "source broke the exactly-n contract");
    }
    // `report.nulls <= n <= u16::MAX`, checked above.
    let pacer_nulls = u16::try_from(report.nulls).unwrap_or(req.n);
    buf.clear();
    buf.extend_from_slice(
        &ResponseHeader {
            status: STATUS_OK,
            seq: req.seq,
            n: req.n,
            pacer_nulls,
        }
        .encode(),
    );
    for p in out.iter() {
        buf.extend_from_slice(p.as_bytes());
    }
    if let Err(e) = c.stream.write_all(buf) {
        // The pacer already advanced: that airtime is lost, and the
        // flowgraph null-fills it.
        eprintln!("oob-mux seam: write failed: {e}");
        return Step::Closed;
    }
    stats.requests.fetch_add(1, Ordering::Relaxed);
    stats.packets.fetch_add(u64::from(req.n), Ordering::Relaxed);
    stats
        .pacer_nulls
        .fetch_add(u64::from(pacer_nulls), Ordering::Relaxed);
    Step::Served
}

/// Replies `status = 1` (best effort), counts and logs, and closes.
fn refuse(c: &mut Conn, stats: &SeamStats, seq: u32, why: &str) -> Step {
    stats.bad_requests.fetch_add(1, Ordering::Relaxed);
    eprintln!("oob-mux seam: {why}");
    let header = ResponseHeader {
        status: STATUS_BAD_REQUEST,
        seq,
        n: 0,
        pacer_nulls: 0,
    };
    let _ = c.stream.write_all(&header.encode());
    Step::Closed
}
