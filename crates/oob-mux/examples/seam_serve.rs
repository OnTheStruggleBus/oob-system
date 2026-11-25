//! The seam endpoint over a stub source: the daemon side of the box-free
//! loopback (issue #15 design note §4). Pair it with `seam_pull`, or with
//! the flowgraph's pull block (#36), to produce the F6-2 evidence log.
//!
//! ```text
//! cargo run -p oob-mux --example seam_serve -- \
//!     --socket /tmp/oob-seam.sock --source stub --fill 0.4 --log-every 10s
//! ```
//!
//! Options: `--socket PATH`, `--source stub|null` (default `stub`),
//! `--fill R` (null ratio of the stub, default 0.4), `--max-pull N`
//! (default 128), `--rate-bps B` (the stub's packet rate, default
//! 2048000, a parameter until O7), `--log-every Ns` (default 10s),
//! `--duration Ns|Nm|Nh` (default: until interrupted).
//!
//! One plain-text line per interval: the cumulative `requests`,
//! `puller_underruns`, `puller_nulls`, `pacer_nulls` and the null ratio.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use oob_mux::seam::stub::{StubConfig, StubSource};
use oob_mux::seam::{SeamConfig, SeamStats, ShutdownFlag, serve};
use oob_mux::{PacerConfig, PacketRate, PullSource};

fn parse_duration(s: &str) -> Option<Duration> {
    let (num, unit) = s.split_at(s.len().checked_sub(1)?);
    let n: u64 = num.parse().ok()?;
    match unit {
        "s" => Some(Duration::from_secs(n)),
        "m" => Some(Duration::from_secs(n * 60)),
        "h" => Some(Duration::from_secs(n * 3600)),
        _ => None,
    }
}

struct Args {
    socket: PathBuf,
    source: String,
    fill: f64,
    max_pull: usize,
    rate_bps: u64,
    log_every: Duration,
    duration: Option<Duration>,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        socket: PathBuf::from("/tmp/oob-seam.sock"),
        source: "stub".into(),
        fill: 0.4,
        max_pull: 128,
        rate_bps: 2_048_000,
        log_every: Duration::from_secs(10),
        duration: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--socket" => a.socket = PathBuf::from(val()?),
            "--source" => a.source = val()?,
            "--fill" => a.fill = val()?.parse().map_err(|e| format!("--fill: {e}"))?,
            "--max-pull" => a.max_pull = val()?.parse().map_err(|e| format!("--max-pull: {e}"))?,
            "--rate-bps" => a.rate_bps = val()?.parse().map_err(|e| format!("--rate-bps: {e}"))?,
            "--log-every" => {
                a.log_every = parse_duration(&val()?).ok_or("--log-every: use Ns, Nm or Nh")?;
            }
            "--duration" => {
                a.duration = Some(parse_duration(&val()?).ok_or("--duration: use Ns, Nm or Nh")?);
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("seam_serve: {e}");
            return ExitCode::from(2);
        }
    };
    let fill = match args.source.as_str() {
        "stub" => args.fill,
        "null" => 1.0,
        other => {
            eprintln!("seam_serve: --source must be stub or null, not {other}");
            return ExitCode::from(2);
        }
    };
    let Some(packet_rate) = PacketRate::from_bits_per_second(args.rate_bps) else {
        eprintln!("seam_serve: --rate-bps must be non-zero");
        return ExitCode::from(2);
    };
    let source = match StubSource::new(StubConfig {
        packet_rate,
        max_pull: args.max_pull,
        null_ratio: fill,
    }) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("seam_serve: {e}");
            return ExitCode::from(2);
        }
    };
    // One max_pull: the seam takes the pacer's (design note, amendment 2).
    let pacer_cfg = PacerConfig {
        packet_rate,
        max_pull: args.max_pull,
        pull_budget: Duration::from_millis(50),
    };
    let cfg = match SeamConfig::from_pacer(&args.socket, &pacer_cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("seam_serve: {e}");
            return ExitCode::from(2);
        }
    };

    let stats = Arc::new(SeamStats::default());
    let shutdown = Arc::new(ShutdownFlag::new());
    println!(
        "seam_serve: socket={} source={} fill={fill} max_pull={} rate_bps={}",
        args.socket.display(),
        args.source,
        args.max_pull,
        args.rate_bps
    );
    let started = Instant::now();
    let result = thread::scope(|scope| {
        let (s, f) = (Arc::clone(&stats), Arc::clone(&shutdown));
        let log_every = args.log_every;
        let duration = args.duration;
        scope.spawn(move || {
            let mut next_log = started + log_every;
            loop {
                thread::sleep(Duration::from_millis(200));
                if f.is_requested() {
                    return;
                }
                let now = Instant::now();
                let elapsed = now.duration_since(started);
                if duration.is_some_and(|d| elapsed >= d) {
                    println!(
                        "{:>8.0}s FINAL {}",
                        elapsed.as_secs_f64(),
                        s.snapshot().log_line()
                    );
                    f.request();
                    return;
                }
                if now >= next_log {
                    println!(
                        "{:>8.0}s {}",
                        elapsed.as_secs_f64(),
                        s.snapshot().log_line()
                    );
                    next_log += log_every;
                }
            }
        });
        serve_it(&cfg, source, &stats, &shutdown)
    });
    shutdown.request();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("seam_serve: {e}");
            ExitCode::FAILURE
        }
    }
}

fn serve_it<S: PullSource>(
    cfg: &SeamConfig,
    source: S,
    stats: &Arc<SeamStats>,
    shutdown: &ShutdownFlag,
) -> Result<(), oob_mux::SeamError> {
    serve(cfg, source, Arc::clone(stats), shutdown)
}
