//! Box-free tests of the pull seam (issue #15, F6-2): the endpoint
//! against the reference puller over a real Unix socket, with the stub
//! pacer as the source.

#[path = "../examples/support/puller.rs"]
mod puller;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use oob_mux::seam::proto::{
    PACKET_SIZE_V1, RESPONSE_HEADER_LEN, Request, ResponseHeader, STATUS_BAD_REQUEST,
};
use oob_mux::seam::stub::{StubConfig, StubSource};
use oob_mux::{
    PacerConfig, PacketRate, PullError, PullReport, PullSource, SeamConfig, SeamError, SeamStats,
    ShutdownFlag, TsPacket,
};
use puller::{Delivery, PACKET, Puller, PullerConfig};

const MAX_PULL: u16 = 16;

fn rate() -> PacketRate {
    // Test fixture only; the real rate is a runtime parameter (O7 open).
    PacketRate::from_bits_per_second(2_048_000).unwrap_or_else(|| unreachable!())
}

fn stub(null_ratio: f64) -> StubSource {
    StubSource::new(StubConfig {
        packet_rate: rate(),
        max_pull: usize::from(MAX_PULL),
        null_ratio,
    })
    .unwrap_or_else(|e| panic!("{e}"))
}

fn socket_path(tag: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    // Short on purpose: macOS caps AF_UNIX paths near 104 bytes.
    std::env::temp_dir().join(format!(
        "oob-{tag}-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

struct Endpoint {
    path: PathBuf,
    stats: Arc<SeamStats>,
    shutdown: Arc<ShutdownFlag>,
    thread: Option<JoinHandle<Result<(), SeamError>>>,
}

impl Endpoint {
    fn start<S: PullSource + 'static>(path: PathBuf, source: S) -> Self {
        Self::start_with(path, source, MAX_PULL)
    }

    fn start_with<S: PullSource + 'static>(path: PathBuf, source: S, max_pull: u16) -> Self {
        let stats = Arc::new(SeamStats::default());
        let shutdown = Arc::new(ShutdownFlag::new());
        let cfg = SeamConfig {
            socket_path: path.clone(),
            max_pull,
            packet_size: PACKET_SIZE_V1,
        };
        let (s, f) = (Arc::clone(&stats), Arc::clone(&shutdown));
        let thread = thread::spawn(move || oob_mux::seam::serve(&cfg, source, s, &f));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() {
            assert!(Instant::now() < deadline, "endpoint never bound");
            thread::sleep(Duration::from_millis(2));
        }
        Self {
            path,
            stats,
            shutdown,
            thread: Some(thread),
        }
    }

    fn stop(&mut self) -> Result<(), SeamError> {
        self.shutdown.request();
        match self.thread.take() {
            Some(t) => t.join().unwrap_or_else(|_| panic!("serve panicked")),
            None => Ok(()),
        }
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn puller_cfg(path: &std::path::Path, n: u16, timeout: Duration) -> PullerConfig {
    PullerConfig {
        socket_path: path.to_path_buf(),
        n,
        reply_timeout: timeout,
        reconnect_interval: Duration::from_millis(20),
    }
}

fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(2));
    }
}

/// Checks sync bytes and, for non-null PIDs, continuity counters.
struct StreamCheck {
    last_cc: HashMap<u16, u8>,
}

impl StreamCheck {
    fn new() -> Self {
        Self {
            last_cc: HashMap::new(),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        assert_eq!(bytes.len() % PACKET, 0, "whole packets only");
        for p in bytes.as_chunks::<PACKET>().0 {
            assert_eq!(p[0], 0x47, "every packet starts with the sync byte");
            let pid = (u16::from(p[1] & 0x1F) << 8) | u16::from(p[2]);
            if pid == 0x1FFF {
                continue;
            }
            let cc = p[3] & 0x0F;
            if let Some(prev) = self.last_cc.insert(pid, cc) {
                assert_eq!(cc, (prev + 1) & 0x0F, "continuity broke on PID {pid:#06x}");
            }
        }
    }
}

#[test]
fn nominal_loopback_has_zero_underruns() {
    let mut ep = Endpoint::start(socket_path("nom"), stub(0.4));
    let mut p = Puller::new(puller_cfg(&ep.path, 8, Duration::from_secs(10)));
    let mut check = StreamCheck::new();
    let mut out = Vec::new();
    let requests = 100_000u64;
    for i in 0..requests {
        out.clear();
        let d = p.pull(&mut out);
        assert!(matches!(d, Delivery::Served { .. }), "request {i}: {d:?}");
        assert_eq!(out.len(), 8 * PACKET);
        check.feed(&out);
    }
    assert_eq!(p.underruns(), 0);
    assert_eq!(p.puller_nulls(), 0);
    assert_eq!(p.stale_discarded(), 0);
    assert_eq!(p.packets(), requests * 8);
    let ratio = p.null_ratio();
    assert!((ratio - 0.4).abs() < 0.02, "null ratio {ratio}");

    let s = ep.stats.snapshot();
    assert_eq!(s.requests, requests);
    assert_eq!(s.packets, requests * 8);
    assert_eq!(s.puller_underruns, 0);
    assert_eq!(s.bad_requests, 0);
    assert_eq!(s.pacer_nulls, p.pacer_nulls());
    assert!((s.null_ratio() - ratio).abs() < 1e-9);
    eprintln!("nominal loopback: {}", s.log_line());
    ep.stop().unwrap_or_else(|e| panic!("{e}"));
}

/// A source that blocks on chosen calls until the test lets it go.
struct SlowSource {
    inner: StubSource,
    calls: u64,
    block_on: Vec<u64>,
    gate: Receiver<()>,
}

impl PullSource for SlowSource {
    fn pull(&mut self, n: usize, out: &mut Vec<TsPacket>) -> Result<PullReport, PullError> {
        self.calls += 1;
        if self.block_on.contains(&self.calls) {
            let _ = self.gate.recv_timeout(Duration::from_secs(30));
        }
        self.inner.pull(n, out)
    }
}

#[test]
fn starvation_nulls_only_the_starved_requests_and_discards_late_replies() {
    let (release, gate) = mpsc::channel();
    let source = SlowSource {
        inner: stub(0.4),
        calls: 0,
        block_on: vec![4, 8],
        gate,
    };
    let ep = Endpoint::start(socket_path("slow"), source);
    let mut p = Puller::new(puller_cfg(&ep.path, 4, Duration::from_millis(150)));
    let mut out = Vec::new();
    let mut deliveries = Vec::new();
    for i in 1..=12u64 {
        out.clear();
        let d = p.pull(&mut out);
        deliveries.push(d);
        assert_eq!(out.len(), 4 * PACKET);
        if d == Delivery::Timeout {
            // The whole request was null-filled by the puller.
            assert!(
                out.as_chunks::<PACKET>()
                    .0
                    .iter()
                    .all(|c| c[1] == 0x1F && c[2] == 0xFF)
            );
            // Let the endpoint finish the abandoned request; its reply is
            // now stale and must be discarded by seq.
            release.send(()).unwrap_or_else(|e| panic!("{e}"));
            wait_until("the abandoned request to be answered", || {
                ep.stats.snapshot().requests == i
            });
        }
    }
    for (idx, d) in deliveries.iter().enumerate() {
        let call = idx + 1;
        if call == 4 || call == 8 {
            assert_eq!(*d, Delivery::Timeout, "call {call}");
        } else {
            assert!(matches!(d, Delivery::Served { .. }), "call {call}: {d:?}");
        }
    }
    assert_eq!(p.underruns(), 2);
    assert_eq!(p.puller_nulls(), 8);
    assert_eq!(p.stale_discarded(), 2);
    // The request after each starvation carried the cumulative count.
    assert_eq!(ep.stats.snapshot().puller_underruns, 2);
    assert_eq!(ep.stats.snapshot().puller_nulls, 8);
}

#[test]
fn absence_nulls_until_the_endpoint_appears_then_serves() {
    let path = socket_path("abs");
    let mut p = Puller::new(puller_cfg(&path, 4, Duration::from_secs(5)));
    let mut out = Vec::new();
    for _ in 0..5 {
        out.clear();
        assert_eq!(p.pull(&mut out), Delivery::Absent);
        assert_eq!(out.len(), 4 * PACKET);
    }
    assert_eq!(p.underruns(), 5);
    assert_eq!(p.puller_nulls(), 20);

    let ep = Endpoint::start(path, stub(0.4));
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut first_served = false;
    while !first_served {
        assert!(Instant::now() < deadline, "never connected");
        out.clear();
        match p.pull(&mut out) {
            Delivery::Served { .. } => first_served = true,
            Delivery::Absent => thread::sleep(Duration::from_millis(5)),
            other => panic!("unexpected {other:?} while the endpoint was coming up"),
        }
    }
    let underruns_at_connect = p.underruns();
    for _ in 0..200 {
        out.clear();
        assert!(matches!(p.pull(&mut out), Delivery::Served { .. }));
    }
    assert_eq!(
        p.underruns(),
        underruns_at_connect,
        "no underruns once connected"
    );
    assert_eq!(ep.stats.snapshot().requests, 201);
}

#[test]
fn a_second_puller_is_refused_and_the_first_is_unaffected() {
    let ep = Endpoint::start(socket_path("two"), stub(0.4));
    let mut a = Puller::new(puller_cfg(&ep.path, 4, Duration::from_secs(5)));
    let mut out = Vec::new();
    assert!(matches!(a.pull(&mut out), Delivery::Served { .. }));

    let mut b = UnixStream::connect(&ep.path).unwrap_or_else(|e| panic!("{e}"));
    b.set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|e| panic!("{e}"));
    let mut byte = [0u8; 1];
    match b.read(&mut byte) {
        Ok(0) => {}
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {}
        other => panic!("the second connection should be closed, got {other:?}"),
    }
    wait_until("the refusal to be counted", || {
        ep.stats.snapshot().refused_connections == 1
    });

    for _ in 0..50 {
        out.clear();
        assert!(matches!(a.pull(&mut out), Delivery::Served { .. }));
    }
    assert_eq!(a.underruns(), 0);
    let s = ep.stats.snapshot();
    assert_eq!((s.connections, s.refused_connections), (1, 1));

    // When the first puller goes, a new one is accepted.
    drop(a);
    let mut c = Puller::new(puller_cfg(&ep.path, 4, Duration::from_secs(5)));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "replacement never connected");
        out.clear();
        if matches!(c.pull(&mut out), Delivery::Served { .. }) {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn expect_refusal(path: &std::path::Path, request: &[u8], seq: u32) {
    let mut s = UnixStream::connect(path).unwrap_or_else(|e| panic!("{e}"));
    s.set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|e| panic!("{e}"));
    s.write_all(request).unwrap_or_else(|e| panic!("{e}"));
    let mut hb = [0u8; RESPONSE_HEADER_LEN];
    s.read_exact(&mut hb).unwrap_or_else(|e| panic!("{e}"));
    let h = ResponseHeader::decode(&hb).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(h.status, STATUS_BAD_REQUEST);
    assert_eq!((h.n, h.pacer_nulls, h.seq), (0, 0, seq));
    let mut rest = [0u8; 1];
    assert_eq!(
        s.read(&mut rest).unwrap_or(0),
        0,
        "connection closes after a refusal"
    );
}

fn good_request() -> Request {
    Request {
        seq: 77,
        n: 4,
        packet_size: PACKET_SIZE_V1,
        puller_underruns: 0,
        puller_nulls: 0,
    }
}

#[test]
fn malformed_and_over_limit_requests_are_refused_with_status_one() {
    let ep = Endpoint::start(socket_path("bad"), stub(0.4));

    let mut bad_magic = good_request().encode();
    bad_magic[0] = b'X';
    expect_refusal(&ep.path, &bad_magic, 0);

    let mut bad_version = good_request().encode();
    bad_version[4] = 2;
    expect_refusal(&ep.path, &bad_version, 0);

    let zero = Request {
        n: 0,
        ..good_request()
    }
    .encode();
    expect_refusal(&ep.path, &zero, 0);

    let over = Request {
        n: MAX_PULL + 1,
        ..good_request()
    }
    .encode();
    expect_refusal(&ep.path, &over, 77);

    let wrong_size = Request {
        packet_size: 204,
        ..good_request()
    }
    .encode();
    expect_refusal(&ep.path, &wrong_size, 77);

    let mut reserved_flags = good_request().encode();
    reserved_flags[6] = 1;
    expect_refusal(&ep.path, &reserved_flags, 0);

    assert_eq!(ep.stats.snapshot().bad_requests, 6);
    assert_eq!(ep.stats.snapshot().requests, 0);
    // Still serving afterwards.
    let mut p = Puller::new(puller_cfg(&ep.path, 4, Duration::from_secs(5)));
    let mut out = Vec::new();
    assert!(matches!(p.pull(&mut out), Delivery::Served { .. }));
}

#[test]
fn a_source_that_refuses_the_pull_gets_status_one_not_a_hang() {
    // The seam accepts up to 16 but this source's own max_pull is 8: the
    // single-max_pull rule exists to prevent exactly this drift.
    let source = StubSource::new(StubConfig {
        packet_rate: rate(),
        max_pull: 8,
        null_ratio: 0.4,
    })
    .unwrap_or_else(|e| panic!("{e}"));
    let ep = Endpoint::start_with(socket_path("src"), source, 16);
    let req = Request {
        n: 12,
        ..good_request()
    }
    .encode();
    expect_refusal(&ep.path, &req, 77);
    assert_eq!(ep.stats.snapshot().bad_requests, 1);
}

#[test]
fn a_partial_request_is_completed_not_refused() {
    let ep = Endpoint::start(socket_path("part"), stub(0.4));
    let mut s = UnixStream::connect(&ep.path).unwrap_or_else(|e| panic!("{e}"));
    s.set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_else(|e| panic!("{e}"));
    let bytes = good_request().encode();
    s.write_all(&bytes[..10]).unwrap_or_else(|e| panic!("{e}"));
    thread::sleep(Duration::from_millis(60)); // several poll intervals
    s.write_all(&bytes[10..]).unwrap_or_else(|e| panic!("{e}"));
    let mut hb = [0u8; RESPONSE_HEADER_LEN];
    s.read_exact(&mut hb).unwrap_or_else(|e| panic!("{e}"));
    let h = ResponseHeader::decode(&hb).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((h.status, h.seq, h.n), (0, 77, 4));
    let mut body = vec![0u8; h.body_len(PACKET_SIZE_V1)];
    s.read_exact(&mut body).unwrap_or_else(|e| panic!("{e}"));
    assert!(body.as_chunks::<PACKET>().0.iter().all(|p| p[0] == 0x47));
}

#[test]
fn a_live_endpoint_blocks_a_second_bind_and_a_stale_socket_is_replaced() {
    let path = socket_path("bind");
    let live = Endpoint::start(path.clone(), stub(0.4));
    let cfg = SeamConfig {
        socket_path: path.clone(),
        max_pull: MAX_PULL,
        packet_size: PACKET_SIZE_V1,
    };
    let second = oob_mux::seam::serve(
        &cfg,
        stub(0.4),
        Arc::new(SeamStats::default()),
        &ShutdownFlag::new(),
    );
    assert!(matches!(second, Err(SeamError::Bind(_))), "{second:?}");
    drop(live);

    // A crashed endpoint leaves its socket file behind.
    drop(UnixListener::bind(&path).unwrap_or_else(|e| panic!("{e}")));
    assert!(path.exists());
    let mut ep = Endpoint::start(path.clone(), stub(0.4));
    let mut p = Puller::new(puller_cfg(&path, 4, Duration::from_secs(5)));
    let mut out = Vec::new();
    // The stale file already "exists", so the endpoint may not have
    // replaced it yet: retry until it has.
    wait_until("the endpoint to replace the stale socket", || {
        out.clear();
        matches!(p.pull(&mut out), Delivery::Served { .. })
    });
    ep.stop().unwrap_or_else(|e| panic!("{e}"));
    assert!(!path.exists(), "the socket file is removed on shutdown");
}

#[test]
fn unusable_settings_are_refused_before_binding() {
    let path = socket_path("cfg");
    let zero = SeamConfig {
        socket_path: path.clone(),
        max_pull: 0,
        packet_size: PACKET_SIZE_V1,
    };
    let r = oob_mux::seam::serve(
        &zero,
        stub(0.4),
        Arc::new(SeamStats::default()),
        &ShutdownFlag::new(),
    );
    assert!(matches!(r, Err(SeamError::BadConfig(_))));
    let odd = SeamConfig {
        packet_size: 204,
        ..zero
    };
    let r = oob_mux::seam::serve(
        &odd,
        stub(0.4),
        Arc::new(SeamStats::default()),
        &ShutdownFlag::new(),
    );
    assert!(matches!(r, Err(SeamError::BadConfig(_))));
    assert!(!path.exists());
}

#[test]
fn seam_max_pull_comes_from_the_pacer() {
    let pc = |max_pull| PacerConfig {
        packet_rate: rate(),
        max_pull,
        pull_budget: Duration::from_millis(1),
    };
    let ok = SeamConfig::from_pacer("/x", &pc(128)).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((ok.max_pull, ok.packet_size), (128, 188));
    let big = SeamConfig::from_pacer("/x", &pc(70_000));
    assert!(matches!(
        big,
        Err(SeamError::MaxPullTooLarge { max_pull: 70_000 })
    ));
}

#[test]
fn the_rust_side_does_no_55_1_coding() {
    // F6-2: 55-1 coding, interleaving and randomization are the
    // flowgraph's. A cheap tripwire over the code (not the comments).
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/seam");
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{e}")) {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
        for line in text.lines().filter(|l| !l.trim_start().starts_with("//")) {
            let l = line.to_lowercase();
            for word in [
                "interleav",
                "scrambl",
                "randomiz",
                "reed",
                "solomon",
                "convolution",
            ] {
                assert!(!l.contains(word), "{}: {line}", path.display());
            }
        }
    }
}

#[test]
fn a_regular_file_at_the_socket_path_is_not_deleted() {
    // oob-qa F-1: on Linux a connect to a regular file is ECONNREFUSED,
    // which must not read as "stale socket".
    let path = socket_path("file");
    std::fs::write(&path, b"precious").unwrap_or_else(|e| panic!("{e}"));
    let r = oob_mux::seam::serve(
        &SeamConfig {
            socket_path: path.clone(),
            max_pull: MAX_PULL,
            packet_size: PACKET_SIZE_V1,
        },
        stub(0.4),
        Arc::new(SeamStats::default()),
        &ShutdownFlag::new(),
    );
    assert!(matches!(r, Err(SeamError::Bind(_))), "{r:?}");
    assert_eq!(std::fs::read(&path).unwrap_or_default(), b"precious");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_socket_is_created_mode_0660() {
    // oob-qa F-2
    use std::os::unix::fs::PermissionsExt;
    let ep = Endpoint::start(socket_path("mode"), stub(0.4));
    let mode = std::fs::metadata(&ep.path)
        .unwrap_or_else(|e| panic!("{e}"))
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o660);
}

#[test]
fn a_wedged_half_sent_request_does_not_hold_the_slot_forever() {
    // oob-qa F-3
    let ep = Endpoint::start(socket_path("wedge"), stub(0.4));
    let mut wedged = UnixStream::connect(&ep.path).unwrap_or_else(|e| panic!("{e}"));
    wedged
        .write_all(&good_request().encode()[..5])
        .unwrap_or_else(|e| panic!("{e}"));
    wedged
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap_or_else(|e| panic!("{e}"));
    // The endpoint drops it after the partial-frame timeout (2 s).
    let mut byte = [0u8; 1];
    match wedged.read(&mut byte) {
        Ok(0) => {}
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {}
        other => panic!("the wedged connection should be dropped, got {other:?}"),
    }
    let mut p = Puller::new(puller_cfg(&ep.path, 4, Duration::from_secs(5)));
    let mut out = Vec::new();
    wait_until("a replacement puller to be served", || {
        out.clear();
        matches!(p.pull(&mut out), Delivery::Served { .. })
    });
}
