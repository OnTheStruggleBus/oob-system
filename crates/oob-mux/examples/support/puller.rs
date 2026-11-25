//! Reference puller: the flowgraph block's seam behaviour in Rust, for the
//! `seam_pull` example and the box-free tests (issue #15 design note §2,
//! §4). It is a model of the GNU Radio source block (#36), not part of the
//! library: it works on raw bytes and makes its own null packets, exactly
//! as the block does, so the library's "null packets exist only in the
//! pacer" rule stays intact.
//!
//! Behaviour modelled: one request outstanding; reply timeout → emit `n`
//! nulls itself and count an underrun; a late reply is recognised by its
//! seq and discarded whole (a half-read frame is kept and finished later);
//! absence or a broken connection → nulls at the caller's pace with a
//! reconnect about once per `reconnect_interval`; the two cumulative
//! counters ride in every request.
#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use oob_mux::seam::proto::{
    PACKET_SIZE_V1, RESPONSE_HEADER_LEN, Request, ResponseHeader, STATUS_OK,
};

pub const PACKET: usize = PACKET_SIZE_V1 as usize;

#[derive(Debug, Clone)]
pub struct PullerConfig {
    pub socket_path: PathBuf,
    /// Packets per request.
    pub n: u16,
    pub reply_timeout: Duration,
    pub reconnect_interval: Duration,
}

/// What one `pull` delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// The endpoint's reply, `pacer_nulls` of which are pacer fill.
    Served { pacer_nulls: u16 },
    /// No reply in time; `n` nulls were made locally.
    Timeout,
    /// Not connected; `n` nulls were made locally.
    Absent,
    /// The connection broke or the endpoint refused; `n` nulls locally.
    Dropped,
}

pub struct Puller {
    cfg: PullerConfig,
    conn: Option<UnixStream>,
    inbuf: Vec<u8>,
    seq: u32,
    last_attempt: Option<Instant>,
    underruns: u32,
    nulls: u32,
    pacer_nulls: u64,
    packets: u64,
    stale_discarded: u64,
}

fn null_packet() -> [u8; PACKET] {
    let mut p = [0xFFu8; PACKET];
    p[0] = 0x47;
    p[1] = 0x1F;
    p[2] = 0xFF;
    p[3] = 0x10;
    p
}

enum Parsed {
    NeedMore,
    Frame(u16),
    Broken,
}

impl Puller {
    pub fn new(cfg: PullerConfig) -> Self {
        Self {
            cfg,
            conn: None,
            inbuf: Vec::new(),
            seq: 0,
            last_attempt: None,
            underruns: 0,
            nulls: 0,
            pacer_nulls: 0,
            packets: 0,
            stale_discarded: 0,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.conn.is_some()
    }

    /// Requests the puller null-filled itself.
    pub fn underruns(&self) -> u32 {
        self.underruns
    }

    /// Packets the puller null-filled itself.
    pub fn puller_nulls(&self) -> u32 {
        self.nulls
    }

    /// Pacer fill summed over served replies.
    pub fn pacer_nulls(&self) -> u64 {
        self.pacer_nulls
    }

    /// Packets emitted in all, served or local.
    pub fn packets(&self) -> u64 {
        self.packets
    }

    /// Late replies read off the socket and thrown away.
    pub fn stale_discarded(&self) -> u64 {
        self.stale_discarded
    }

    /// (pacer nulls + own nulls) / packets emitted: the F6-2 null ratio.
    pub fn null_ratio(&self) -> f64 {
        if self.packets == 0 {
            0.0
        } else {
            (self.pacer_nulls + u64::from(self.nulls)) as f64 / self.packets as f64
        }
    }

    pub fn log_line(&self) -> String {
        format!(
            "packets={} puller_underruns={} puller_nulls={} pacer_nulls={} stale_discarded={} null_ratio={:.4}",
            self.packets,
            self.underruns,
            self.nulls,
            self.pacer_nulls,
            self.stale_discarded,
            self.null_ratio()
        )
    }

    /// Appends exactly `n × 188` bytes to `out`: the endpoint's packets,
    /// or local nulls when it could not answer in time.
    pub fn pull(&mut self, out: &mut Vec<u8>) -> Delivery {
        let n = usize::from(self.cfg.n);
        let start = out.len();
        let delivery = match self.try_pull(out) {
            Ok(pacer_nulls) => {
                self.pacer_nulls += u64::from(pacer_nulls);
                Delivery::Served { pacer_nulls }
            }
            Err(why) => {
                out.truncate(start);
                let np = null_packet();
                for _ in 0..n {
                    out.extend_from_slice(&np);
                }
                self.underruns = self.underruns.wrapping_add(1);
                self.nulls = self.nulls.wrapping_add(u32::from(self.cfg.n));
                why
            }
        };
        self.packets += n as u64;
        delivery
    }

    fn drop_conn(&mut self) {
        self.conn = None;
        self.inbuf.clear();
        self.last_attempt = Some(Instant::now());
    }

    fn try_pull(&mut self, out: &mut Vec<u8>) -> Result<u16, Delivery> {
        if self.conn.is_none() {
            let due = self
                .last_attempt
                .is_none_or(|t| t.elapsed() >= self.cfg.reconnect_interval);
            if !due {
                return Err(Delivery::Absent);
            }
            self.last_attempt = Some(Instant::now());
            match UnixStream::connect(&self.cfg.socket_path) {
                Ok(s) => {
                    self.conn = Some(s);
                    self.inbuf.clear();
                }
                Err(_) => return Err(Delivery::Absent),
            }
        }
        self.seq = self.seq.wrapping_add(1);
        let req = Request {
            seq: self.seq,
            n: self.cfg.n,
            packet_size: PACKET_SIZE_V1,
            puller_underruns: self.underruns,
            puller_nulls: self.nulls,
        };
        let sent = match self.conn.as_mut() {
            Some(c) => c.write_all(&req.encode()),
            None => return Err(Delivery::Absent),
        };
        if sent.is_err() {
            self.drop_conn();
            return Err(Delivery::Dropped);
        }
        let deadline = Instant::now() + self.cfg.reply_timeout;
        loop {
            match self.parse(out) {
                Parsed::Frame(pacer_nulls) => return Ok(pacer_nulls),
                Parsed::Broken => {
                    self.drop_conn();
                    return Err(Delivery::Dropped);
                }
                Parsed::NeedMore => {}
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Delivery::Timeout);
            }
            let Some(c) = self.conn.as_mut() else {
                return Err(Delivery::Absent);
            };
            if c.set_read_timeout(Some(remaining)).is_err() {
                self.drop_conn();
                return Err(Delivery::Dropped);
            }
            let mut tmp = [0u8; 8192];
            match c.read(&mut tmp) {
                Ok(0) => {
                    self.drop_conn();
                    return Err(Delivery::Dropped);
                }
                Ok(k) => self.inbuf.extend_from_slice(&tmp[..k]),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(Delivery::Timeout);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => {
                    self.drop_conn();
                    return Err(Delivery::Dropped);
                }
            }
        }
    }

    /// Consumes whole frames from the buffer: stale ones are discarded, the
    /// one for the current seq is appended to `out`.
    fn parse(&mut self, out: &mut Vec<u8>) -> Parsed {
        loop {
            if self.inbuf.len() < RESPONSE_HEADER_LEN {
                return Parsed::NeedMore;
            }
            let mut hb = [0u8; RESPONSE_HEADER_LEN];
            hb.copy_from_slice(&self.inbuf[..RESPONSE_HEADER_LEN]);
            let Ok(h) = ResponseHeader::decode(&hb) else {
                return Parsed::Broken;
            };
            if h.status != STATUS_OK {
                return Parsed::Broken;
            }
            let frame = RESPONSE_HEADER_LEN + h.body_len(PACKET_SIZE_V1);
            if self.inbuf.len() < frame {
                return Parsed::NeedMore;
            }
            if h.seq != self.seq {
                self.inbuf.drain(..frame);
                self.stale_discarded += 1;
                continue;
            }
            if h.n != self.cfg.n {
                return Parsed::Broken;
            }
            out.extend_from_slice(&self.inbuf[RESPONSE_HEADER_LEN..frame]);
            self.inbuf.drain(..frame);
            return Parsed::Frame(h.pacer_nulls);
        }
    }
}
