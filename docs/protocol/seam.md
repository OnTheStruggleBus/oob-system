# The pull seam: mux ↔ PHY contract (protocol v1)

How the GNU Radio flowgraph pulls transport packets from the daemon's pacer.
Source of truth: `oob_mux::seam` (`cargo doc -p oob-mux`); this page is the
contract both sides build against. Built by issue #15 to ADR-002 decision 2
and r6 F6-2 (both accepted 2026-11-22); the transport is Marshal's ruling
of 2026-11-23 on the #15 design note. The flowgraph side is #36
(hardware-blocked), which implements the block against this page.

This is Oob's own protocol between two of its processes, not a box wire
format, so it has no `documented` / `hypothesized` / `verified` status. One
field inside it does: the **188-byte packet size** is the r4 MPEG-2 lead and
stays `hypothesized` until a capture verifies it (see
[the mux pipeline](oob-mux-pipeline.md)). That is why the size is carried on
the wire: a surprise from O7 is a version bump, not a silent break.

## Roles

- **The flowgraph is the clock master.** A custom GNU Radio source block
  owns timing; it asks for packets when its downstream buffer has room.
- **The endpoint answers.** `oob_mux::seam::serve` runs on one dedicated
  OS thread (blocking `std` sockets, no tokio in the data path), calls
  `PullSource::pull`, and replies. oob-mux stays a library; the oob-dac
  daemon spawns the thread (#29).
- **The seam's unit is the 188-byte transport packet.** SCTE 55-1 coding,
  interleaving and randomization stay in the flowgraph (F6-2). Nothing on
  the Rust side touches them (a test greps for it).

## Transport

An `AF_UNIX` `SOCK_STREAM` socket, one request outstanding at a time.
Local-only by construction, and bounded: the response is sized by the
request, so the channel holds at most one reply. `SOCK_STREAM` rather than
`SEQPACKET` or `DGRAM` because macOS has no `SEQPACKET` and its datagram
limit is smaller than a reply, and plain `std` sockets (and Python's
`socket`) work on Linux and macOS alike. ZMQ REQ/REP was not chosen: the
flowgraph needs custom null-on-timeout code either way, and a ZMQ REQ
socket cannot send again after a missed reply without being rebuilt.

Socket path: `/run/oob/seam.sock` by default, a Layer 1 install setting
(ADR-003); the socket is created mode 0660. In a deployment the directory
is bind-mounted into the rootless container and group-owned by the
flowgraph service's group (ADR-002 decision 6, F6-9); that mapping is an
ops task, not part of this contract. On start the endpoint replaces a stale
socket file left by a crash, but refuses to bind over a live endpoint, and
never removes a path that is not a socket (a misconfigured path fails the
bind instead of deleting a file).

## Wire format

All integers are little-endian. `seam::proto` is the pure encoder/decoder;
the endpoint, the reference puller and the tests all use it.

**Request, flowgraph → endpoint, 24 bytes:**

| Offset | Field | Type | Meaning |
| --- | --- | --- | --- |
| 0 | magic | `[u8;4]` | `b"OOBP"` |
| 4 | version | u16 | `1` |
| 6 | flags | u16 | `0`, reserved; non-zero is refused |
| 8 | seq | u32 | increments per request, wraps |
| 12 | n | u16 | packets requested, `1..=max_pull` |
| 14 | packet_size | u16 | `188` (`hypothesized`, see above) |
| 16 | puller_underruns | u32 | flowgraph's cumulative count of requests it null-filled itself (timeout or absence) |
| 20 | puller_nulls | u32 | flowgraph's cumulative count of packets it null-filled itself |

**Response, endpoint → flowgraph, 16-byte header then `n × packet_size` bytes:**

| Offset | Field | Type | Meaning |
| --- | --- | --- | --- |
| 0 | magic | `[u8;4]` | `b"OOBR"` |
| 4 | version | u16 | `1` |
| 6 | status | u16 | `0` OK; `1` bad request (the endpoint then closes) |
| 8 | seq | u32 | echo of the request seq (`0` if the request could not be parsed) |
| 12 | n | u16 | equals the requested `n` when status = 0; `0` when status = 1 |
| 14 | pacer_nulls | u16 | how many of the `n` packets are pacer null fill |

The body is exactly `n` packets of `packet_size` bytes, each starting with
the sync byte 0x47. A status-1 response has no body.

## Semantics

- **Exactly n.** The endpoint always answers with exactly `n` packets,
  taken from `PullSource::pull`, which null-fills any shortfall. Null
  packets exist only in the pacer (F6-2); the endpoint never makes one.
- **No puller, no airtime.** While nothing is connected the source is not
  pulled, so stream time stands still. That is correct: no clock master
  means no airtime.
- **Reply timeout (flowgraph side).** The block waits at most
  `reply_timeout` for a reply; default no more than half the airtime of its
  downstream buffer, a block parameter and not a constant. On timeout it
  emits `n` null packets itself, increments `puller_underruns` and
  `puller_nulls`, and goes on. A reply to an earlier request arrives later
  carrying an old `seq`: the block reads it off the socket in full and
  discards it, and a frame left half-read by a timeout is finished before
  the next one is parsed.
- **Absence (flowgraph side).** If connect fails or the connection drops,
  the block emits null packets at its own pace and retries about once a
  second. Each `work()` call served that way counts toward
  `puller_underruns` and `puller_nulls`. The carrier never gaps (ADR-002
  Consequences): a gap would read as `E 01` on the box.
- **One puller.** The endpoint serves a single connection. A second
  concurrent connection is accepted only to be closed, logged and counted
  (`refused_connections`), because two pullers would drain the pacer
  twice. The first connection is unaffected. When it ends, the next
  connection is accepted; a successor that connects in the same accept
  batch as the close is counted refused and retries. A request left
  half-sent for more than two seconds drops the connection, so a wedged
  peer cannot hold the slot.
- **Refusal.** A bad magic, version, flags, `n = 0`, `n > max_pull` or
  wrong `packet_size` gets a status-1 header and a closed connection; so
  does a source that refuses the pull (`PullError`) or breaks the
  exactly-n contract. The event is logged and counted (`bad_requests`). The
  flowgraph treats this as absence and null-fills, so the carrier never
  gaps.
- **One `max_pull`.** The seam's `max_pull` is the pacer's
  (`SeamConfig::from_pacer`), a single Layer 1 value, so the seam cannot
  accept a request the pacer would refuse.
- **Null ratio** for the G0.5 log is (`pacer_nulls` summed over responses
  + `puller_nulls`) / total packets emitted. Both terms are logged
  separately, so pacer fill (normal) can be told from seam failure (an
  underrun). The endpoint keeps the puller's counters as the last
  cumulative value it reported, so they lag by one request and reset if the
  flowgraph restarts.

## Config versus code (ADR-003)

| Layer | What |
| --- | --- |
| Code | frame layout, magic, version, the exactly-n rule, the single-puller rule, `packet_size` 188 (any change is a protocol version bump through a PR) |
| Install settings (Layer 1) | socket path, `max_pull` (from the pacer config) |
| Flowgraph block parameters, versioned with the `.grc` (#36) | `n` per request, `reply_timeout`, the packet rate (from the flowgraph's own coding chain, until O7) |

## Counters

`SeamStats` is plain atomics; `/metrics` exposure is #29.

| Counter | Meaning |
| --- | --- |
| `requests`, `packets` | OK replies and the packets in them |
| `puller_underruns`, `puller_nulls` | the flowgraph's last reported cumulative values (the F6-2 seam-failure counters) |
| `pacer_nulls` | pacer null fill summed over replies: normal operation |
| `bad_requests` | requests refused with status 1 |
| `connections`, `refused_connections` | pullers accepted; second connections closed |

## Box-free validation harness (F6-2)

No RF, no box. Two examples in `crates/oob-mux/examples/` form a loopback
over a Unix socket, so the seam can be proven before the PHY exists:

- `seam_serve` serves the seam over `StubSource`: a real pacer behind a real
  scheduler with synthetic cyclic sections on private PIDs, so the nulls are
  the pacer's own and the continuity counters the packetizer's. `--fill`
  sets the null ratio (steps of 0.01). It logs one plain-text line per
  interval.
- `seam_pull` is the reference puller, a Rust model of the flowgraph block
  (timeout, absence, stale-seq discard, cumulative counters). It paces
  itself at the stream rate as the clock master, checks the sync byte and
  continuity counters, and exits non-zero on any underrun.

```text
cargo run --release -p oob-mux --example seam_serve -- \
    --socket /tmp/oob-seam.sock --source stub --fill 0.4 --log-every 30s --duration 200s
cargo run --release -p oob-mux --example seam_pull -- \
    --socket /tmp/oob-seam.sock --n 16 --rate-bps 2048000 --duration 180s --log-every 30s
```

The rate is a parameter until O7: 2048000 b/s is the OOB stream's aggregate
rate (ADR-001) used as a representative value, not a claim about the packet
framing.

CI runs the same behaviour as nextest tests (`crates/oob-mux/tests/seam.rs`):
a nominal run of 10⁵ requests with zero underruns, every packet starting
with 0x47 and continuity counters intact; a starvation test that blocks the
source on chosen requests and asserts nulls and an underrun on exactly those
(late replies discarded by seq, the next request served normally); absence
and recovery; the second-puller refusal; and the malformed-request cases.

### Representative run

Release build, macOS dev host, 180 s, `n = 16`, stub fill 0.4, rate
2048000 b/s (airtime 11.75 ms per request), reply timeout 250 ms. Final
lines of the two logs:

```text
seam_pull   180s FINAL packets=245120 puller_underruns=0 puller_nulls=0 pacer_nulls=98040 stale_discarded=0 null_ratio=0.4000 late_requests=0 cc_breaks=0 sync_losses=0
seam_pull   RESULT: PASS (zero underruns)
seam_serve  200s FINAL requests=15320 packets=245120 puller_underruns=0 puller_nulls=0 pacer_nulls=98040 bad_requests=0 connections=1 refused=0 null_ratio=0.4000
```

Zero underruns, zero late requests, no continuity or sync break, and a null
ratio of 0.4000 that is all pacer fill (`puller_nulls = 0`). This is a
short representative run on a dev host, not G0.5 evidence; the evidence run
is the 24 h step below.

### The 24 h run is a separate ops step

The G0.5 exit needs a **24 h loopback with zero underruns** (F6-2). That
is not run inside the build task. It is an ops step that pairs with #36,
which is hardware-blocked: the same 24 h window, on the bench host, with the
real GNU Radio block pulling from `seam_serve` (or the daemon), logs from
both sides committed as G0.5 evidence, and the F5-10/F6-2 exit criteria
checked off by Marshal at the gate. Until then the harness above is the
short representative run. To run the box-free 24 h variant on any host:

```text
seam_serve --duration 24h30m --log-every 5m   # endpoint first
seam_pull  --duration 24h    --log-every 5m   # then the clock master
```

`seam_pull` prints `RESULT: PASS` and exits 0 only with zero underruns and
no continuity or sync break. Closed plant only when the real block drives
the HackRF (AGENTS.md §7): dummy load or terminated loopback RX.
