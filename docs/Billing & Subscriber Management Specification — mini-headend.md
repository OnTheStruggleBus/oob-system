# Billing & Subscriber Management Specification — mini-headend

- **Date:** 2026-11-22
- **Status:** Proposed — awaits Marshal's disposition (gate-text amendments in §7 are proposals, not accepted changes)
- **Owner:** Marshal (@Marshal)
- **Companion to:** VOD & PPV Simulation Specification (S4), Management Console Specification, Project Milestones & Stage Gates (G6), DAC-6000 Operations Reference

---

## 1. The gap — and the fun

The corpus now covers boot, authorization, tiers, downloads, the guide,
the return path, and (proposed) the transactional product layer. What is
still missing is the **subscriber relationship**: accounts, rate cards,
monthly statements, payments, late notices, and the non-pay disconnect
that makes the whole addressable-control edifice worth building. In the
period plant this was the business system's job — CSG, ICOMS, and their
ilk — and the DAC 6000 existed, in large part, so that *billing decisions
could reach the box*: "managing service databases and automated scheduling
of services, including subscriptions" and collecting "impulse transactions
from terminals" for the business system (DAC manual, §1-4).

There is also a social reality: the plant's subscribers are the operator's
friends, and the evidence so far is that **they enjoy the billing theater
as much as the service itself** — getting a monthly cable bill for a
fictitious cable system, mailing back a fictitious payment, arguing with a
fictitious rate increase. This spec leans into that. Everything here is
**obviously and deliberately fictitious**: no real money moves, no real
payment instruments are accepted, no real debt exists, and every rendered
document carries that label by default. Within that label, we are
period-faithful: the account structures, statement layout, service codes,
and disconnect/reconnect choreography are modeled on how the real systems
behaved, because the period behavior is the point.

This spec defines the simulation, the data model, and — per Marshal's
explicit requirement — **a first-class, fully documented Billing API**.
**No new crates** (AGENTS.md §4 naming freeze): the work lands in the Oob
Console's business-system role and couples to `oob-dac` strictly through
the interfaces that already exist or are already proposed (§3.1). **No
build is scheduled by this document** — gate amendments are Marshal's to
accept.

## 2. What the real system did (evidence base)

### 2.1 The DAC↔billing split

The DAC 6000 did not bill. It kept the unit database and the entitlement
state; the **business system** — a separate computer from a separate
vendor — kept accounts, money, and the product catalog, and drove the DAC
over the **Business System Interface (BSI)**: WireLink ports (RS-232
and/or TCP) carrying the 600/700-series command set (VOD/PPV spec §2.2;
DAC manual §4). The DAC supported **multiple business systems
concurrently**, each with its own handles and its own PPV Configuration
Mode (Table 4-1: Standard / Standard+Zero Handle / Master BS /
Master BS+Zero Handle).

The billing-relevant traffic over that interface:

- **Service authorization changes.** When a subscriber's service mix
  changed — new install, upgrade, downgrade, non-pay disconnect,
  pay-to-restore — the business system told the DAC, and the DAC issued
  the authorization operations to the terminal. The decision lived in
  billing; the execution lived in the DAC.
- **PPV program downloads with handles** (call-ahead) and **IPPV purchase
  uploads** (694 / 620–622 / 720–726 command IDs; VOD/PPV spec §2.2) —
  purchases were born at the box, collected by the DAC, and handed to the
  business system to *be rated and billed*. The purchase only became money
  when billing posted it.
- **Account linkage.** The DAC's Define Terminal schema carries an
  **account field** per terminal (Operations Reference; DAC manual §9) —
  the foreign key that ties a 40-bit unit address to a billing account.
  One account could have many terminals; a terminal belonged to exactly
  one account.

### 2.2 The period billing product model

We do not hold a CSG or ICOMS manual (see §10) — the product model below
is therefore **styled period-authentic from the DAC-side evidence plus
well-documented industry practice, and is labeled as such**. The
structures every cable billing system of the era shared:

- **Account** — the billable entity: account number, name, service
  address, billing cycle, status, balance.
- **Service codes** — the sellable units (basic tier, expanded tier,
  premium multiplexes, equipment rental, outlet fees), each with a
  **rate code** resolving to a price on a **rate card**, versioned by
  effective date (period rate increases were events; ours can be too).
- **The mapping that matters:** each billing service code resolves to a
  set of **DAC-side authorizations** (services/programs/packages, VOD/PPV
  spec §3). Billing owns what the subscriber *pays for*; the DAC owns
  what the box *does*.
- **The cycle:** recurring charges post on the cycle date → statement
  renders → payment window → late notices → **soft disconnect**
  (authorizations pulled except a lifeline minimum) → **hard disconnect**
  (terminal deactivated) → restore-on-payment.

### 2.3 Disconnect as a billing action

Non-pay disconnect is the reason addressability existed as a business
proposition: no truck roll, no trap on the pole — the business system
flips the account, the DAC de-authorizes the terminal, and the box
becomes a clock. The DAC's operations catalog (Operations Reference, from
DAC manual §11) provides the execution primitives: authorization changes
per terminal, bulk operations with filters, and the Disconnect operation
family. Restore is the same path in reverse. **This spec's central
integration rule: billing state changes drive DAC operations through the
same BSI/admin path a real business system would have used — never
through a back door into the unit DB** (F5-12: unit DB = derived state;
oob-dac's API is the only writer).

### 2.4 What we are *not* reproducing

- Real payment processing, card data, bank interfaces, credit checks,
  collections, or anything in PCI scope. Payments are operator-posted
  fictitious ledger entries (§3.6).
- A real billing vendor's internals (no CSG/ICOMS manual in hand —
  acquisition item O16; their screen layouts and exact workflows are
  out of evidence and therefore out of scope).
- Real taxation: statement tax/fee lines are configurable fictitious
  percentages for period flavor, computed for theater, not compliance.
- Any subscriber who has not explicitly opted in, and any personal data
  beyond what a subscriber volunteers (aliases encouraged — §3.2).

## 3. Simulation design

### 3.1 Placement — the Console's business-system role, expanded

Period fidelity forbids putting billing *inside* the DAC reproduction:
the DAC↔business-system split was load-bearing. The VOD/PPV spec (§6)
already established the reproduction of **both ends**: `oob-dac` hosts
the WireLink 600/700-series BSI server endpoint, and the **Oob Console
plays the business system** over it. This spec expands that
business-system role from PPV transactions into the full subscriber
lifecycle:

- **oob-dac** — unchanged in principle: unit DB, entitlements, scheduler,
  BSI endpoint. It learns nothing about money. Billing reaches it only
  through (a) BSI commands (PPV program/purchase traffic) and (b) the
  WireLink-verb admin API (authorization changes, disconnect ops) — the
  same two paths a period business system had.
- **Oob Console (business-system role)** — owns the billing schema
  (accounts, rate cards, ledger, statements) and exposes the **Billing
  API** (§5). This is the Console's second face, alongside the operator
  front-end; the purchase ledger (VOD/PPV spec §4) already set the
  precedent that the business-system role owns state.
- **No new crate, no new component name.** The Console spec's phase
  plan (C0–C5) absorbs the UI surfaces; proposed phase amendments are in
  §7 and remain Marshal's to accept.

### 3.2 Accounts and the subscriber lifecycle

States: `pending` → `active` ⇄ `soft_disconnect` ⇄ `active`,
`active` → `hard_disconnect` → `closed` (terminal-level states mirror
per unit; account state is the roll-up).

Lifecycle events, each producing a dated, immutable history entry:

1. **Install** — account created, terminals linked by 40-bit unit
   address (the DAC account field is populated to match — the same
   cross-check the period systems relied on), service codes attached,
   welcome statement rendered.
2. **Change of service** — upgrade/downgrade: effective-dated service
   code delta → DAC authorization operation via admin API → confirmed
   in ledger and history.
3. **Move / premise change** — service address update; fictitious
   premises are fine and expected.
4. **Non-pay disconnect / restore** — §3.7.
5. **Voluntary disconnect** — close to `hard_disconnect`, final
   statement, balance written off or "refunded" (fictitiously).

**Privacy rule:** accounts may use aliases and fictitious addresses; the
schema never requires legal names. Subscribers are friends who opted in;
the schema stores no government identifiers, no real card numbers, no
real bank data — ever (system-prompt privacy rules apply as code rules).

### 3.3 Service codes, rate codes, and the rate card

```
service_code   SVC-DIGBASIC   "Digital Basic"
               SVC-DIGPLUS    "Digital Value Tier"
               SVC-HBO        "HBO Multiplex"
               SVC-BOXRENT    "Converter rental, additional outlet"
rate_code      R-DB-2001      → service SVC-DIGBASIC, $39.95/mo,
                               effective 2001-01-01..2001-12-31
rate_card      named set of rate codes, versioned, effective-dated
```

Each service code carries its **entitlement mapping** — the DAC-side
services/programs/packages it authorizes (per VOD/PPV spec §3's
entitlement model). Rate cards are versioned and effective-dated so a
**period rate increase** can be scheduled as an event: new rate code
version, advance notice line on the prior statement, the works. PPV
events are *not* service codes — they arrive rated from the purchase
ledger (§3.8) and post as one-time charges.

### 3.4 The ledger

Double-entry-flavored, **immutable, append-only**: every money movement
is a ledger entry with a type (`recurring_charge`, `ppv_charge`,
`one_time_charge`, `payment`, `adjustment_credit`, `adjustment_debit`,
`late_fee`, `writeoff`), an amount, an effective date, a statement
reference once billed, and a free-text memo. Corrections are **reversing
entries**, never edits — period systems kept audit trails, and so do we.
Account balance is always derivable: `sum(debits) − sum(credits)`. The
ledger is the single source of truth the statement renderer, the dunning
engine, and the API all read.

### 3.5 Statements

Monthly, on the account's cycle day:

- Rendered to **PDF styled as a period cable bill** — remittance stub,
  previous balance / payments / current charges / total due box, line
  items by service code, PPV purchases itemized with event title and
  epoch, fictitious tax and franchise-fee lines, message-of-the-month
  slot (rate-increase notices, channel lineup changes — the full theater).
- **Brand skin comes from the deployment config slot** (operator-agnostic
  naming rule): the statement template has slots for operator name, logo
  block, remit-to address, and customer-service phone. No preset operator
  identity ships; "ArchaiTel" appears nowhere.
- **Fictitious labeling is a first-class template slot, default ON:** a
  visible "FICTITIOUS STATEMENT — NOT A BILL — NO AMOUNT IS OWED" band.
  It can be styled but not silently removed; turning it off is a
  deliberate deployment config change, logged.
- Statements are retained per account, addressable via the API, and
  re-renderable (renderer is deterministic given ledger + rate card
  versions, so historical statements reproduce byte-for-byte).

### 3.6 Payments (fictitious)

Operator-posted only: the subscriber "mails a check," the operator posts
a `payment` entry with a method label (`check`, `money_order`,
`cash_drop`) chosen for flavor. There is **no payment gateway, no card
field anywhere in the schema, no PCI surface**. Posting rules: payments
apply oldest-first; an overpayment carries as a credit balance (shown on
the next statement, because a negative cable bill is period comedy gold).
A payment sufficient to clear the past-due amount triggers the restore
workflow (§3.7) without operator intervention — the business system's
most satisfying automation.

### 3.7 Dunning and the non-pay disconnect

Configurable schedule (defaults in days after cycle): statement → day 10
friendly reminder line on next message slot → day 20 **late fee** posts
(fictitious) → day 30 **soft disconnect**: billing calls the admin API to
pull every service code's entitlement mapping except the configured
lifeline set (e.g., barker/guide stay — the box still shows the grid, it
just can't watch much, exactly like the period soft disconnect) → day 60
**hard disconnect**: terminal deactivation op. **Payment reverses it:**
post a clearing payment and the restore op queue fires within the
configured batch interval, re-authorizing per the account's service
codes, with a history entry at each step. Every billing-driven DAC
operation is recorded bidirectionally: billing history references the DAC
operation ID; the DAC operation's memo references the billing event ID.
The Console shows both sides.

### 3.8 PPV purchases on the statement

The VOD/PPV purchase ledger (call-ahead bookings, impulse uploads,
orphans) is the source: each settled purchase becomes a `ppv_charge`
ledger entry on the terminal's account — event title, epoch, price,
BSG handle, and terminal nicknamed by account. This closes the loop the
period systems had: the box makes the purchase, the DAC collects it, the
business system bills it, the subscriber reads it on the statement and
pays for it (fictitiously). NVOD stagger instances bill as their parent
event.

## 4. Data model (sketch)

```yaml
account:
  account_number: string        # generated, period-styled (e.g. 4-8-4 grouped digits)
  display_name: string          # alias-friendly
  service_address: string       # fictitious premises welcome
  cycle_day: int                # 1..28
  status: enum                  # pending/active/soft_disconnect/hard_disconnect/closed
  brand_skin_ref: string|null   # per-account override; default = deployment slot
  created_at / closed_at: timestamp

terminal_link:                  # many per account, one account per terminal
  unit_address: uint40          # the DAC foreign key (matches Define Terminal account field)
  nickname: string              # "living room", per account
  linked_at / unlinked_at: timestamp

service_code / rate_code / rate_card:    # §3.3, versioned + effective-dated
account_service:                # service_code attached to account, with dates
  effective_from / effective_to: date

ledger_entry:                   # append-only, §3.4
  id / account_id / type / amount_cents / effective_date
  statement_id: ref|null
  dac_operation_id: ref|null    # set when the entry drove a DAC op
  memo: string

statement:
  id / account_id / cycle_date / rendered_pdf_ref
  opening_balance / closing_balance / due_date
  line_items: derived view over ledger entries

payment: ledger_entry(type=payment) + method_label + posted_by
dunning_campaign: schedule config (thresholds, late-fee amount, lifeline set)
history_event: account_id / kind / detail / at / actor
```

All money in integer cents. All timestamps UTC. The schema lives in the
Console business-system role's database, versioned by migrations, and is
exclusively accessed through the Billing API.

## 5. The Billing API

Marshal's requirement: **a well-documented API.** It gets the same
engineering treatment as the oob-dac admin API, end to end.

### 5.1 Documentation contract

- **OpenAPI 3.1**, generated from code annotations (utoipa, same pattern
  as the admin API) — the spec file is a build artifact, never
  hand-edited.
- **CI drift check:** the committed `billing-openapi.json` must equal the
  generated one; PRs that change endpoints without regenerating fail
  (same drift-check rule as the admin API).
- **mdBook chapter** in the docs site: the rendered OpenAPI plus a
  concepts guide (account lifecycle, ledger semantics, dunning state
  machine) with worked examples for every resource — the chapter is
  written for the friends as much as for operators.
- **Every field documented:** each schema property carries a description
  and an example; enum values are individually explained; fictitious-only
  fields say so in their descriptions.

### 5.2 Conventions

- **Base:** `/api/billing/v1` — version in the path; breaking changes
  bump the version, old versions stay live through a deprecation window.
- **JSON**, UTF-8; timestamps RFC 3339; money as integer cents with the
  currency field fixed to the fictitious unit (`"USD-FICTION"`).
- **Errors:** RFC 9457 problem-details envelope
  (`type`, `title`, `status`, `detail`, `instance`) with a stable
  machine-readable `code` (e.g. `account_not_found`,
  `ledger_immutable`, `dac_op_failed`) — error codes are enumerated in
  the OpenAPI doc.
- **Idempotency:** all POSTs accept an `Idempotency-Key` header; the
  server deduplicates on (key, account) for 24h — posting a payment twice
  must be impossible.
- **Pagination:** cursor-based (`limit`, `cursor`), consistent shape
  (`items`, `next_cursor`) on every collection.
- **Filtering/sorting:** allowlisted query params per collection,
  documented per endpoint; no arbitrary field injection.
- **Auth:** operator token for write paths; per-account **subscriber
  tokens** (read-only, own account only) so friends can fetch their own
  statements and balance — the "check my bill" self-service surface.
- **Events:** an `events` outbox stream (account_created,
  statement_rendered, payment_posted, dunning_step, entitlement_op_sent,
  entitlement_op_confirmed) — the nightly canary and the Console both
  consume it.
- **Redaction:** API responses never contain DAC secrets; unit addresses
  are the only hardware identifiers exposed, and only to operator tokens.

### 5.3 Resource map

| Resource | Endpoints (v1) | Notes |
| --- | --- | --- |
| Accounts | `POST/GET /accounts`, `GET/PATCH /accounts/{id}`, `POST /accounts/{id}/close` | lifecycle per §3.2 |
| Terminal links | `PUT/DELETE /accounts/{id}/terminals/{unit_address}` | cross-checks DAC account field via admin API |
| Service codes & rate cards | `GET /service-codes`, `GET/POST /rate-cards`, `GET /rate-cards/{id}/versions` | rate-card versioning read-mostly |
| Account services | `PUT /accounts/{id}/services/{code}` with effective dates | drives DAC auth ops on activation |
| Ledger | `GET /accounts/{id}/ledger`, `POST /accounts/{id}/ledger/adjustments` | entries immutable; only reversing adjustments are writable |
| Payments | `POST /accounts/{id}/payments`, `GET /accounts/{id}/payments` | operator-posted; idempotent; triggers restore when clearing |
| Statements | `GET /accounts/{id}/statements`, `GET /statements/{id}`, `GET /statements/{id}/pdf` | deterministic re-render |
| Dunning | `GET /accounts/{id}/dunning`, `GET/PUT /dunning/config` | current step + schedule config |
| Events | `GET /events` (cursor stream) | outbox, §5.2 |
| Self-service | `GET /self/account`, `GET /self/statements`, `GET /self/balance` | subscriber-token scope |

### 5.4 Relationship to the admin API and BSI

Three separate surfaces, three separate jobs: the **admin API**
(oob-dac's, WireLink verbs) is the DAC's operator interface; the **BSI**
(oob-dac-hosted server) carries period business-system traffic (PPV
programs/purchases); the **Billing API** (this spec) is the business
system's own face to operators and subscribers. Billing → DAC calls flow
*outbound* from the business-system role into the admin API (auth ops,
disconnect) and over the BSI (purchase traffic) — the same separation of
concerns the period vendors shipped.

## 6. Console integration (UI surfaces)

Operator front-end additions (absorbed into the Console phase plan, §7):
accounts list with status badges and balance; account detail (services,
terminals, ledger, statements, dunning timeline, history); payment
posting form; rate-card editor with effective-date scheduling; statement
preview; dunning dashboard (who's at which step, which DAC ops are
queued). Subscriber self-service page (statement list + PDF download +
balance + the friendly fictitious-disclaimer footer) — the friends'
front door, themed by the deployment's brand skin.

## 7. Gate and milestone mapping — PROPOSED (Marshal dispositions)

Registered stretch: **S5 — Billing & subscriber management** (theater
layer; software-only, no box dependency, no RF at all).

| Item | Proposed host | Proposed text |
| --- | --- | --- |
| S5 registration | Stretch register | S5 = billing/subscriber management per this spec; re-scored at G6 review like S1–S4 |
| Billing core (accounts, rate cards, ledger, statements, Billing API + docs) | **G6** checklist item | One account lifecycle end-to-end (install → statement → payment) demonstrable against the canary box's account; API OpenAPI doc green in CI with drift check |
| Dunning coupling | **G6** checklist item | A scripted non-pay cycle on the canary account: soft disconnect pulls entitlements (box capture shows denial), fictitious payment restores them (box capture shows recovery) — the billing theater's signature demo |
| Console surfaces | Console phases (proposed C-phase additions) | accounts/payments/statements/dunning journeys pass without SSH |
| Subscriber self-service | **G6** checklist item | A subscriber token fetches that account's statement PDF and balance only |

Nothing here moves W1–W8 scope, the G0.5 agenda, or the G1 critical path;
the schema and API are buildable any time after Console C0 exists, exactly
like the BSI endpoint's software-only track (VOD/PPV spec §7).

## 8. Evidence plan (checkpoints)

1. **Schema + migrations green**; property tests on ledger invariants
   (append-only, balance = derived, reversing entries net out).
2. **API contract green:** OpenAPI generation + drift check in CI;
   mdBook chapter builds; example requests from the docs chapter run
   against a seeded fixture in CI.
3. **Statement determinism:** fixed fixture → byte-identical PDF across
   rebuilds (hash-pinned).
4. **DAC coupling (post-G1):** service-code activation issues the right
   authorization ops; dunning soft-disconnect pulls them; capture shows
   the canary box denied, then restored after the fictitious payment —
   two captures, labeled, in the evidence pack.
5. **PPV loop (post-S4 P2):** an impulse purchase lands as a `ppv_charge`
   on the right statement with title/epoch/price matching the purchase
   ledger.
6. **Nightly canary extension:** one synthetic account walks
   install→statement→payment every night in software; the event outbox
   shows the full sequence.

## 9. Explicit non-goals

- No real money, payment instruments, gateways, or PCI scope — payments
  are operator-posted fictitious ledger entries, period.
- No involuntary subscribers and no unvolunteered personal data; aliases
  and fictitious premises are first-class.
- No real tax computation; statement tax/fee lines are configurable
  theater.
- No credit checks, no collections, no legal anything.
- No changes to W1–W8 scope, gate G0.5's agenda, or the G1 critical path.
- No new crates, no new component names (AGENTS.md §4); no preset
  operator identity (naming revision, 2026-11-22) — brand skin stays a
  deployment config slot.
- No billing logic inside oob-dac: the DAC↔business-system split is
  reproduced, not collapsed.

## 10. Open items

- **O16** — Period billing-system reference acquisition (CSG/ICOMS-era
  operator manuals, period cable-bill scans for the statement template):
  acquisition items, Marshal upload pattern (restricted docs are not
  bypassed). Until then the product model is labeled period-styled
  (§2.2) and the statement template is an original design in period
  style, not a reproduction of a specific artifact.
- **O17** — Subscriber onboarding policy: how friends opt in, alias
  conventions, who holds operator tokens, and the house rules for
  fictitious late fees (social contract, not software — but the dunning
  config defaults should encode whatever Marshal decides).
- **O18** — Brand-skin pack for statements: needs the deployment config
  slot populated; default template ships with a clearly generic
  placeholder identity, consistent with the operator-agnostic naming rule.
