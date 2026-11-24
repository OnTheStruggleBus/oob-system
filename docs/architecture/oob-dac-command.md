# oob-dac command layer

Every operation enters `oob-dac` through one command layer. It implements
r6 F6-11 (one command layer; campaign pacing here and nowhere else;
adapters over it) under ADR-002 decision 1 (every operation enters through
one internal command layer), decision 7 (lab injection is C5 through the
same path) and action item 5 (the first Slice D module, before any
adapter). Code: `crates/oob-dac/src/command/` (issue #22).

The REST API (#27), the WireLink-wire listener (#28), billing, the
Discovery Lab and the DAC's own timers are **adapters**: each holds a
`CommandBus` and calls `CommandBus::submit`. The bus is also the **only**
path into the `oob-mux` scheduler.

## Modules

| Module | Role |
| --- | --- |
| `catalog` | the DAC §11 per-terminal operations, the Define Terminal actions, and the Define Operations user-defined and auto-generated kinds, each with a support status |
| `target` | All / Terminal-ID range / Single plus the Plan §2 filters, over a `TerminalDirectory` (store-backed or in-memory fake) |
| `lifecycle` | the operation status state machine |
| `campaign` | release-time planning: delay of at least 250 ms, Repeat-Every |
| `policy` | the hook; `policy::StorePolicy` ([the policy layer](oob-dac-policy.md)) implements it |
| `exec` | encoding under `catch_unwind` |
| `sink` (private) | the one place a scheduler handle is held |

## Pipeline

One path for every front door.

1. **Validate.** Catalog support first (an unsupported entry is rejected by
   name, with its reason), then parameters: `delay_between` of at least
   250 ms, and a Repeat-Every period of at least one campaign.
2. **Resolve** the targets through the directory. An empty selection is an
   error (`empty-target`), never a silent no-op; an unknown `Single` ID is
   `unknown-terminal`.
3. **Policy.** `Policy::check` receives the command and the resolved
   terminals. A dry run stops here and returns a `DryRunReport`; nothing is
   created and nothing reaches `oob-mux`.
4. **Plan** release times from the bus clock: terminal *i* of *n* releases
   at `t0 + i × delay_between`. A single-terminal operation releases at
   `t0`.
5. **Encode** every PDU of the repetition under `catch_unwind` and keep
   them in the operation's pending queue (an encode failure therefore never
   leaves a half-submitted campaign).
6. **Release just in time.** `CommandBus::tick` hands each PDU to the
   scheduler as a one-shot `Submission`, with the `Pdu.release_at` and
   `deadline` the command layer set, only when its release time comes
   within `BusConfig::release_window` (default 1 s). The daemon ticks
   at least that often. A campaign is never held in the scheduler's
   per-class queue all at once, so its size is not capped by `queue_depth`,
   and cancel, failure and end time stop the feed.
7. **Track.** The daemon (#29) drains the scheduler's outcome channel into
   `CommandBus::on_outcome`, which drives the lifecycle.

### Spacing exists only as release times

Campaign spacing is expressed **only** as `Pdu.release_at` on one-shot
PDUs. `oob-mux` never sees a campaign; its C1 class honours the times it is
given and never infers spacing (#16). The 250 ms minimum is `documented`
(DAC manual, Define Operations; Plan §2) and is a code constant, not a
setting. A 50-terminal campaign at 250 ms therefore yields 50 PDUs whose
release times are exactly 250 ms apart, and 249 ms is rejected with
`delay-below-minimum` before anything is created.

A Repeat-Every period shorter than `terminals × delay` is rejected
(`repeat-too-short`): a shorter period would let repetition *k + 1* start
before repetition *k* finished and put PDUs for different terminals closer
than the minimum. Each repetition re-plans the whole set from its slot
(`rep start + period`). If a tick is late, a PDU whose release time has
already passed is re-timed to keep at least `delay_between` after the
previous one, so spacing never shrinks.

**A full scheduler queue.** `QueueFull` is retried a few times at once and
then once per tick, with the release time preserved. If the queue stays
full, the operation fails (Timeout while still Request Sent, Failed
afterwards) and its pending PDUs are dropped, so nothing more is submitted.
At submit time a full queue is refused before anything is queued.

**Lost outcomes.** The scheduler counts an outcome lost when its channel is
full. Each handed-over PDU is therefore tracked with the time its outcome
is overdue (its deadline plus `BusConfig::outcome_grace`); `tick` fails an
operation whose outcome never arrived (`outcome-lost` in the log) instead
of leaving it in Downloaded or Active forever.

### The scheduler is reachable only through the command layer

- `CommandBus::new` takes the `SchedulerHandle` **by value**; the daemon
  moves its handle in. No other `oob-dac` module receives one.
  `SchedulerHandle` is `Clone` and public in `oob-mux`, so the guarantee
  within `oob-dac` is a convention held by the source scan below, not by the
  type. #25's budget apply and #17's assemblers also need a handle; #29
  decides between a bus pass-through and a daemon-held clone.
- The sink module and the encode guard are private. Two `compile_fail`
  doctests show an adapter cannot name them.
- The test `the_scheduler_is_named_only_in_the_command_layer` scans
  `crates/oob-dac/src` and fails if `oob_mux` or `Scheduler` appears outside
  `command/`.

### Encoder panics are caught per command

An **encoder** panic is caught around each encode (`exec::encode_guarded`),
never per process (ADR-002 Consequences). The command fails with
`encoder-panic`, its operation is left in Request Sent Failed, the event is
logged (`CommandBus::log`) and the bus carries on; a test runs a panicking
command concurrently with a good one. Panics in a `Policy`, a
`TerminalDirectory` or a `NonPduHandler` are not caught here.

A command that becomes an operation and then fails returns
`CommandError::OperationFailed`, which carries the operation's ID (and
`reason()` looks through it), so the caller can read its status and log. A
failure found by `tick` (a later repetition, a lost outcome) has no caller;
it is recorded in the operation's status and the log. This relies on `panic = "unwind"`,
the default; no Oob build profile may set `panic = "abort"`.

## Status lifecycle

The status **names and definitions** are `documented`: *Basic DAC 6000
(V3.1) Operation, Rev 1.2*, p. 11-4, recorded in the DAC Reference §10.
They describe an event on the **DAC to RADD** exchange, not a terminal's
reply. Oob plays the DAC and RADD roles in one daemon, so the "RADD" is the
path from the executor through the `oob-mux` scheduler to the PHY seam.

The **transition order** and the **Oob meaning** of each status are
`hypothesized`: p. 11-4 is a list, not a transition table. They live in one
data table (`lifecycle::TRANSITIONS`), so a page-confirmed correction
changes the table and its tests, not the pipeline.

| Status | Oob meaning (`hypothesized`) | Final |
| --- | --- | --- |
| New | accepted; start time or Repeat-Every slot not reached | no |
| Request Sent | planning and encoding started; PDUs being handed to the scheduler | no |
| Request Sent Failed | encoder error or panic, or the scheduler refused for a reason other than capacity | yes |
| Timeout | the scheduler did not accept the PDUs within the bound (`QueueFull` retries exhausted) | yes |
| Downloaded | the plan is accepted: the first release window is handed to the scheduler and the rest waits in the command layer's release queue (accepted, not yet sent) | no |
| Active | the first PDU has been emitted; emission in progress | no |
| Delayed | waiting while a Category Epoch operation is active | no |
| Completed | the last PDU of the final repetition was emitted | yes |
| Marked Completed | an Active operation passed its end time | yes |
| Failed | a PDU expired or was dropped, or the operation was cancelled | yes |

Main path: New, Request Sent, then Request Sent Failed, Timeout or
Downloaded, then Active, then Completed, Failed or Marked Completed.
Repeat-Every: each repetition goes Active, Downloaded, Active. Every other
pair is an illegal transition; `every_pair_is_checked_against_the_table`
tests all 100.

**Overlays.** Retrieving Status, Unknown and Device Offline sit beside the
main status and return to it. They are never final and are cleared when an
operation becomes final.

- *Retrieving Status* is transient: the Retrieve Operation Status control
  (`CommandBus::retrieve_status`) enters and leaves it inside the call.
- *Unknown*: reconciliation finds a non-final operation that the scheduler
  holds no PDUs of and has emitted none of (for example after a restart).
- *Device Offline*: `CommandBus::set_link(false)`, fed from the seam's
  connection state (#15), shows it on non-final operations until the puller
  reconnects.

**One-way terminals.** The lifecycle has no dependency on a terminal reply,
so an operation on a One-Way terminal ends **Completed** like any other
(oob-expert: an inference from the documented definitions, not a quote).
Terminal-side results belong to `terminal_status` (#21).

**Not reachable yet.** Delayed is entered only once Category Epoch
Transition is supported (it is in the catalog as unsupported); until then
the state machine is tested by driving it directly. Cancellation is a
`ControlCommand::Cancel` and fails the operation with no more specific
status. Cancel stops the feed: no further PDU is handed to the scheduler.
PDUs already handed over (at most one release window's worth) cannot be
recalled and will be emitted. Closing a destructive operation early still
sends those few; destructive operations need owner and a token
([the policy layer](oob-dac-policy.md)), and exposing Cancel from REST (#27) or WireLink (#28) should wait on
a one-shot withdraw in `oob-mux` (a #16-side follow-up).

## Catalog and support status

Every entry the manual names is **present**. An entry Oob does not
reproduce is rejected with a **named reason**. The unsupported set is a
design decision for this slice, not a manual statement.

| Set | Unsupported entries and reason |
| --- | --- |
| Terminal operations | Network (DVB): DVB only; Oob reproduces the GI/Motorola headend |
| Define Operations (user-defined) | Category Epoch Transition: not implemented yet; Resource OCAM: needs the OCAM component; Synchronize CPMS: CPMS is a separate billing-side system |
| Define Operations (auto-generated) | Analog Rekey, Analog Clear Service, Macrovision: the analog tier (S3) is unscoped; CableCARD SNT: outside the DCT2000-class fleet |

`Authorize` is a plain variant (the design note's `AuthorizeWhat` sub-enum is
deferred until its cases are sourced), and the eight unsupported entries
above are rejected rather than omitted so that every refusal is
explainable.

*Supported* means the command layer accepts the entry and routes it to a
family encoder (`oob-msg`, #19). Whether that family's wire format is
`documented`, `hypothesized` or `verified` is `oob-msg`'s status, not the
catalog's. Targets are filtered by terminal type, downstream and upstream
plant and Business System (the BSI name) against the store; the RADD and
RPD-channel filters are accepted by the in-memory directory but refused by
name by the store-backed one, because the Define Terminal record carries
neither yet.

## Policy hook (#23)

`Policy::check` runs after validation and target resolution and before
anything is created; `Policy::audit` receives the outcome of every command,
including refusals. `policy::StorePolicy` (#23, [the policy layer](oob-dac-policy.md))
implements both: owner role, the confirmation token bound to (operation,
terminal), the sacrificial-only CDL lock, the dry-run default and guest
refusal (r6 F6-10).

`AllowAllPolicy` is a **test double**. `CommandBus::new` has no default
policy, so an adapter must pass one, and none may ship with the double: an
adapter passes a `StorePolicy`. `DefineOperation::new` builds a dry run, and
`DryRunReport::plan_digest` is what a destructive bulk token is bound to.

## The non-PDU seam (#25)

`Command` and `CommandError` are `#[non_exhaustive]`. A command that
produces no PDU is dispatched to a `NonPduHandler` after the same policy
check. `#[non_exhaustive]` binds other crates; inside `oob-dac` matches stay
exhaustive, so `dispatch` deliberately sends **every command it does not
recognise as a PDU command** down the non-PDU route (a final wildcard arm),
and the default handler refuses what it does not know. #25 adds
`Command::Admin(AdminOp)` for configuration saves by adding the variant and
registering a handler, so owner role, guest refusal and audit from #23 apply
to config saves with no change to `dispatch`. There is one handler slot and
registering replaces the built-in handler (which answers `Ping`), so #25's
handler wraps or delegates to it.
`ControlCommand` (`Ping`, `Cancel`) is the stub that proves the path today;
a doctest and `a_non_pdu_command_is_dispatched_through_the_seam_after_policy`
show it.

## Configuration layer (ADR-003)

| Item | Layer |
| --- | --- |
| catalog sets and support status, the lifecycle table, the 250 ms minimum | 3 (code) |
| per-operation `delay_between`, `repeat_every`, `run_for`, targets, filters | runtime command parameters (Layer 1, audited by #23) |
| `BusConfig` (`pdu_deadline`, `queue_full_retries`) | Layer 1 settings once #25 stores them; code defaults until then |

## Not built yet

- **Persistence and restart-resume.** Operations are tracked in memory. The
  precious `operation` ledger table (F6-5), audit rows and resume after a
  restart need the ledger migration and land with the daemon (#29); #23's
  audit rows already exist. Until then a restart forgets in-flight operations.
- **One-shot withdrawal.** `oob-mux` can withdraw carousels but not queued
  one-shot PDUs. The command layer keeps future-dated PDUs itself and
  releases them just in time, so cancel, failure and end time stop the feed;
  PDUs already handed over (at most one release window's worth) are emitted.
  `BusConfig::pdu_deadline` only drops a PDU whose first packet is still
  waiting after its release time; it does not recall anything. A one-shot
  withdraw in `oob-mux` would close the remaining window (#16-side
  follow-up).
- **Operation IDs** come from an in-memory counter until the ledger lands;
  they must come from the store then, or IDs repeat across restarts.
  Targets are resolved once per operation, so Repeat-Every does not see
  terminals added or removed later.
- **Real encoders.** `oob-msg` (#19) supplies the family encoders behind
  the `Encoder` trait; the tests use doubles.
