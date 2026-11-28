-- 0003_config: the configuration store (issue #25, ADR-003 §2, r6 F6-4,
-- F6-5).
--
-- Every table created here has an entry in store/classify.rs TABLES in the
-- same commit. Forward-only: this file never changes once released.

-- Precious, append-only. One row per revision of a configuration object
-- (ADR-003 Layer 1): the four singletons (lineup, pid_plan, class_budgets,
-- mux_settings) and each unit's mutable operational record. The head of an
-- object is its highest revision; there is no head table to disagree with
-- the history. doc is canonical JSON (sorted keys, no whitespace) and
-- doc_sha256 its digest. audit_seq is the audit_log row (the journal)
-- written in the same transaction.
CREATE TABLE config_revision (
    kind           TEXT    NOT NULL
                   CHECK (kind IN ('lineup', 'pid_plan', 'class_budgets', 'mux_settings',
                                   'unit')),
    -- '' for a singleton; the decimal terminal_id for a unit.
    object_id      TEXT    NOT NULL DEFAULT '',
    revision       INTEGER NOT NULL CHECK (revision >= 1),
    doc            TEXT    NOT NULL CHECK (json_valid(doc)),
    doc_sha256     TEXT    NOT NULL CHECK (length(doc_sha256) = 64),
    doc_schema     INTEGER NOT NULL CHECK (doc_schema >= 1),
    origin         TEXT    NOT NULL CHECK (origin IN ('save', 'rollback', 'seed')),
    rolled_back_to INTEGER CHECK (rolled_back_to IS NULL OR rolled_back_to >= 1),
    actor          TEXT    NOT NULL,
    ts_utc         TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    audit_seq      INTEGER NOT NULL UNIQUE REFERENCES audit_log (seq),
    PRIMARY KEY (kind, object_id, revision),
    CHECK ((kind = 'unit') = (object_id <> '')),
    CHECK (object_id = '' OR object_id NOT GLOB '*[^0-9]*'),
    CHECK ((origin = 'rollback') = (rolled_back_to IS NOT NULL)),
    CHECK (rolled_back_to IS NULL OR rolled_back_to < revision)
) STRICT;

CREATE TRIGGER config_revision_no_update BEFORE UPDATE ON config_revision
BEGIN
    SELECT RAISE(ABORT, 'append-only: config_revision');
END;

CREATE TRIGGER config_revision_no_delete BEFORE DELETE ON config_revision
BEGIN
    SELECT RAISE(ABORT, 'append-only: config_revision');
END;

-- REPLACE deletes the conflicting row (QA-1 of #21): refuse any insert that
-- targets an existing (kind, object_id, revision).
CREATE TRIGGER config_revision_no_overwrite BEFORE INSERT ON config_revision
WHEN EXISTS (SELECT 1 FROM config_revision
             WHERE kind = NEW.kind AND object_id = NEW.object_id
               AND revision = NEW.revision)
BEGIN
    SELECT RAISE(ABORT, 'append-only: config_revision');
END;

-- The terminal table is rebuilt (PR #87 QA-1, per oob-architect ruling):
--
-- * retired_utc: retiring a unit never deletes it; terminal IDs are never
--   reused and the audit trail references them. NULL means in service.
-- * Identity uniqueness covers units in service only. terminal_id is a
--   record's permanent identity; unit_address and serial are unique among
--   live units (partial unique indexes below), so a unit enrolled with a
--   wrong identity is retired and enrolled again correctly. Identity stays
--   immutable per row; a re-enrollment is a new ID.
--
-- The runner executes this in a transaction with foreign_keys=ON, so the
-- rebuild keeps terminal_status (whose FK cascades on the DROP) in a scratch
-- table and carries the AUTOINCREMENT high-water mark, in this order.
CREATE TABLE terminal_new (
    terminal_id            INTEGER PRIMARY KEY AUTOINCREMENT
                           CHECK (terminal_id BETWEEN 1 AND 16777215),
    unit_address           INTEGER NOT NULL
                           CHECK (unit_address BETWEEN 0 AND 1099511627775),
    ua_check               INTEGER NOT NULL CHECK (ua_check BETWEEN 0 AND 999),
    serial                 TEXT NOT NULL CHECK (length(serial) = 12),
    terminal_type          TEXT NOT NULL
                           CHECK (terminal_type IN ('One-Way', 'Fone-Way', 'Two-Way',
                                                    'Ethernet', 'DOCSIS')),
    box_type               TEXT NOT NULL CHECK (length(box_type) > 0),
    account_number         TEXT,
    on_plant               INTEGER NOT NULL DEFAULT 0 CHECK (on_plant IN (0, 1)),
    install_date           TEXT CHECK (install_date IS NULL
                                       OR date(install_date) IS install_date),
    auto_registration_date TEXT CHECK (auto_registration_date IS NULL
                                       OR date(auto_registration_date) IS auto_registration_date),
    activated              INTEGER NOT NULL DEFAULT 0 CHECK (activated IN (0, 1)),
    autodiscovered         INTEGER NOT NULL DEFAULT 0 CHECK (autodiscovered IN (0, 1)),
    ip_address             TEXT CHECK (ip_address IS NULL
                                       OR (length(ip_address) BETWEEN 2 AND 45
                                           AND ip_address NOT GLOB '*[^0-9A-Fa-f.:]*')),
    bsi_name               TEXT,
    downstream_plant       TEXT,
    upstream_plant         TEXT,
    sacrificial            INTEGER NOT NULL DEFAULT 0 CHECK (sacrificial IN (0, 1)),
    created_utc            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    retired_utc            TEXT CHECK (retired_utc IS NULL OR length(retired_utc) > 0)
) STRICT;

INSERT INTO terminal_new (terminal_id, unit_address, ua_check, serial, terminal_type, box_type,
                          account_number, on_plant, install_date, auto_registration_date,
                          activated, autodiscovered, ip_address, bsi_name, downstream_plant,
                          upstream_plant, sacrificial, created_utc, retired_utc)
SELECT terminal_id, unit_address, ua_check, serial, terminal_type, box_type, account_number,
       on_plant, install_date, auto_registration_date, activated, autodiscovered, ip_address,
       bsi_name, downstream_plant, upstream_plant, sacrificial, created_utc, NULL
FROM terminal;

-- Terminal IDs are never reused: carry the high-water mark, which can be
-- above max(terminal_id).
DELETE FROM sqlite_sequence WHERE name = 'terminal_new';
INSERT INTO sqlite_sequence (name, seq)
SELECT 'terminal_new', seq FROM sqlite_sequence WHERE name = 'terminal';

CREATE TABLE terminal_status_keep AS SELECT * FROM terminal_status;

DROP TABLE terminal;
ALTER TABLE terminal_new RENAME TO terminal;

CREATE UNIQUE INDEX terminal_live_unit_address ON terminal (unit_address)
    WHERE retired_utc IS NULL;
CREATE UNIQUE INDEX terminal_live_serial ON terminal (serial)
    WHERE retired_utc IS NULL;

INSERT INTO terminal_status SELECT * FROM terminal_status_keep;
DROP TABLE terminal_status_keep;

-- Precious. oob-mux's per-table PSI version counters and last-emitted
-- section bytes (#17 TableVersions::snapshot), so a restart continues the
-- mod-32 version count instead of re-emitting a changed table at version 0.
-- kind is 'pat', 'cat' or 'pmt:<program_number>' (store::psi_kind; the
-- daemon, #29, writes it). Rows are upserted and never pruned: a retired
-- table keeps its version so a re-add continues the count, and the set is
-- bounded by two plus the program numbers ever used.
CREATE TABLE psi_table_version (
    kind        TEXT    PRIMARY KEY
                CHECK (kind IN ('pat', 'cat')
                       OR (kind GLOB 'pmt:[1-9]*' AND substr(kind, 5) NOT GLOB '*[^0-9]*'
                           AND length(kind) <= 9
                           AND CAST(substr(kind, 5) AS INTEGER) <= 65535)),
    version     INTEGER NOT NULL CHECK (version BETWEEN 0 AND 31),
    bytes       BLOB    NOT NULL,
    retired     INTEGER NOT NULL CHECK (retired IN (0, 1)),
    updated_utc TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
