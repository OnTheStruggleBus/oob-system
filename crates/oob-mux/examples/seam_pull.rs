//! A Rust fake puller with the flowgraph block's seam behaviour (issue #15
//! design note §4): the clock master of the box-free loopback. It paces
//! itself at the stream's packet rate, asks `seam_serve` for `n` packets
//! per request, null-fills on timeout or absence, and logs the F6-2
//! terms: underruns, null counts and the null ratio.
//!
//! ```text
//! cargo run -p oob-mux --example seam_pull -- \
//!     --socket /tmp/oob-seam.sock --n 16 --rate-bps 2048000 \
//!     --duration 3m --log-every 10s
//! ```
//!
//! Options: `--socket PATH`, `--n N` (packets per request, default 16),
//! `--rate-bps B` (default 2048000, a parameter until O7), `--timeout-ms M`
//! (reply timeout, default 250), `--log-every Ns` (default 10s),
//! `--duration Ns|Nm|Nh` (default 1m). Exits 0 only when the run had zero
//! underruns, no continuity break and no sync loss; 1 otherwise.
//!
//! Closed-loop pacing: the next request is due one airtime after the
//! previous one was due, so a slow reply is caught up, not accumulated.
//! A request answered after its due slot is counted `late_requests`.

#[path = "support/puller.rs"]
mod puller;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use puller::{Delivery, PACKET, Puller, PullerConfig};

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
    n: u16,
    rate_bps: u64,
    timeout: Duration,
    log_every: Duration,
    duration: Duration,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        socket: PathBuf::from("/tmp/oob-seam.sock"),
        n: 16,
        rate_bps: 2_048_000,
        timeout: Duration::from_millis(250),
        log_every: Duration::from_secs(10),
        duration: Duration::from_secs(60),
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--socket" => a.socket = PathBuf::from(val()?),
            "--n" => a.n = val()?.parse().map_err(|e| format!("--n: {e}"))?,
            "--rate-bps" => a.rate_bps = val()?.parse().map_err(|e| format!("--rate-bps: {e}"))?,
            "--timeout-ms" => {
                a.timeout = Duration::from_millis(
                    val()?.parse().map_err(|e| format!("--timeout-ms: {e}"))?,
                );
            }
            "--log-every" => {
                a.log_every = parse_duration(&val()?).ok_or("--log-every: use Ns, Nm or Nh")?;
            }
            "--duration" => {
                a.duration = parse_duration(&val()?).ok_or("--duration: use Ns, Nm or Nh")?;
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    if a.n == 0 || a.rate_bps == 0 {
        return Err("--n and --rate-bps must be non-zero".into());
    }
    Ok(a)
}

fn main() -> ExitCode {
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("seam_pull: {e}");
            return ExitCode::from(2);
        }
    };
    // Airtime of one request: n packets of 188 bytes at the stream rate.
    let bits = u64::from(args.n) * (PACKET as u64) * 8;
    let airtime = Duration::from_nanos(bits * 1_000_000_000 / args.rate_bps);
    println!(
        "seam_pull: socket={} n={} rate_bps={} airtime={:?} timeout={:?}",
        args.socket.display(),
        args.n,
        args.rate_bps,
        airtime,
        args.timeout
    );

    let mut p = Puller::new(PullerConfig {
        socket_path: args.socket,
        n: args.n,
        reply_timeout: args.timeout,
        reconnect_interval: Duration::from_secs(1),
    });
    let started = Instant::now();
    let mut due = started;
    let mut next_log = started + args.log_every;
    let mut out = Vec::with_capacity(usize::from(args.n) * PACKET);
    let mut last_cc: HashMap<u16, u8> = HashMap::new();
    let (mut late, mut cc_breaks, mut sync_losses) = (0u64, 0u64, 0u64);

    while started.elapsed() < args.duration {
        let now = Instant::now();
        if due > now {
            thread::sleep(due - now);
        }
        out.clear();
        let delivery = p.pull(&mut out);
        let finished = Instant::now();
        due += airtime;
        if finished > due {
            late += 1;
        }
        if matches!(delivery, Delivery::Served { .. }) {
            for pkt in out.as_chunks::<PACKET>().0 {
                if pkt[0] != 0x47 {
                    sync_losses += 1;
                    continue;
                }
                let pid = (u16::from(pkt[1] & 0x1F) << 8) | u16::from(pkt[2]);
                if pid == 0x1FFF {
                    continue;
                }
                let cc = pkt[3] & 0x0F;
                if let Some(prev) = last_cc.insert(pid, cc)
                    && cc != (prev + 1) & 0x0F
                {
                    cc_breaks += 1;
                }
            }
        } else {
            // Our own nulls break the stream's CC history by design.
            last_cc.clear();
        }
        if finished >= next_log {
            println!(
                "{:>8.0}s {} late_requests={late} cc_breaks={cc_breaks} sync_losses={sync_losses}",
                started.elapsed().as_secs_f64(),
                p.log_line()
            );
            next_log += args.log_every;
        }
    }
    println!(
        "{:>8.0}s FINAL {} late_requests={late} cc_breaks={cc_breaks} sync_losses={sync_losses}",
        started.elapsed().as_secs_f64(),
        p.log_line()
    );
    if p.underruns() == 0 && cc_breaks == 0 && sync_losses == 0 {
        println!("RESULT: PASS (zero underruns)");
        ExitCode::SUCCESS
    } else {
        println!("RESULT: FAIL ({} underruns)", p.underruns());
        ExitCode::FAILURE
    }
}
