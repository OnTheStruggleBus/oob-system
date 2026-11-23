-- 0001_initial: the first oob-dac tables (issue #21, r6 F6-5, ADR-003 §6).
--
-- Every table created here has an entry in store/classify.rs TABLES in the
-- same commit; the registry test fails otherwise. Forward-only: this file
-- never changes once released; later schema changes are 0002_… and up.

-- Precious. The Define Terminal record (Plan §3.2). terminal_id is
-- allocated by the store (24-bit, never reused: AUTOINCREMENT), never
-- supplied, so no box instance can be seeded from git (ADR-003 §6).
-- unit_address is the 40-bit address (the first 13 digits of the printed
-- nnn-nnnnn-nnnnn-nnn form); ua_check holds the last three printed digits.
CREATE TABLE terminal (
    terminal_id            INTEGER PRIMARY KEY AUTOINCREMENT
                           CHECK (terminal_id BETWEEN 1 AND 16777215),
    unit_address           INTEGER NOT NULL UNIQUE
                           CHECK (unit_address BETWEEN 0 AND 1099511627775),
    ua_check               INTEGER NOT NULL CHECK (ua_check BETWEEN 0 AND 999),
    serial                 TEXT NOT NULL UNIQUE CHECK (length(serial) = 12),
    terminal_type          TEXT NOT NULL
                           CHECK (terminal_type IN ('One-Way', 'Fone-Way', 'Two-Way',
                                                    'Ethernet', 'DOCSIS')),
    box_type               TEXT NOT NULL CHECK (length(box_type) > 0),
    account_number         TEXT,
    on_plant               INTEGER NOT NULL DEFAULT 0 CHECK (on_plant IN (0, 1)),
    install_date           TEXT,
    auto_registration_date TEXT,
    activated              INTEGER NOT NULL DEFAULT 0 CHECK (activated IN (0, 1)),
    autodiscovered         INTEGER NOT NULL DEFAULT 0 CHECK (autodiscovered IN (0, 1)),
    ip_address             TEXT,
    bsi_name               TEXT,
    downstream_plant       TEXT,
    upstream_plant         TEXT,
    sacrificial            INTEGER NOT NULL DEFAULT 0 CHECK (sacrificial IN (0, 1)),
    created_utc            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- DerivedObserved. What the plant reports about a terminal; NULL means
-- "not observed yet". Rebuilt with no observations and refilled by polls
-- and d-screen scrapes. The row itself is seeded by the TerminalCreated
-- event (outbox deriver `terminal_status`).
CREATE TABLE terminal_status (
    terminal_id             INTEGER PRIMARY KEY
                            REFERENCES terminal (terminal_id) ON DELETE CASCADE,
    last_response_utc       TEXT,
    failures                INTEGER CHECK (failures >= 0),
    power_leveling_attempts INTEGER CHECK (power_leveling_attempts >= 0),
    non_participating_count INTEGER CHECK (non_participating_count >= 0),
    chronic_non_responding  INTEGER CHECK (chronic_non_responding IN (0, 1)),
    d_screen                TEXT CHECK (d_screen IS NULL OR json_valid(d_screen)),
    observed_utc            TEXT
) STRICT;

-- Precious, append-only. The outbox: every precious change with derived
-- consequences appends here in the same transaction.
CREATE TABLE event_log (
    seq     INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_utc  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    kind    TEXT NOT NULL,
    payload TEXT NOT NULL CHECK (json_valid(payload)),
    actor   TEXT NOT NULL
) STRICT;

CREATE TRIGGER event_log_no_update BEFORE UPDATE ON event_log
BEGIN
    SELECT RAISE(ABORT, 'append-only: event_log');
END;

CREATE TRIGGER event_log_no_delete BEFORE DELETE ON event_log
BEGIN
    SELECT RAISE(ABORT, 'append-only: event_log');
END;

-- DerivedFromEvents. How far each deriver has folded event_log; drop it
-- and replay starts from 0.
CREATE TABLE derive_watermark (
    deriver     TEXT PRIMARY KEY,
    applied_seq INTEGER NOT NULL CHECK (applied_seq >= 0)
) STRICT;

-- Precious, append-only. The bsilog equivalent (DAC Reference §8): one
-- audit path shared by commands (#23, F6-10) and the config journal (#25).
CREATE TABLE audit_log (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    ts_utc     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    actor      TEXT NOT NULL,
    role       TEXT NOT NULL,
    front_door TEXT NOT NULL,
    command    TEXT NOT NULL,
    target     TEXT,
    outcome    TEXT NOT NULL,
    before     TEXT CHECK (before IS NULL OR json_valid(before)),
    after      TEXT CHECK (after IS NULL OR json_valid(after))
) STRICT;

CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'append-only: audit_log');
END;

CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'append-only: audit_log');
END;
