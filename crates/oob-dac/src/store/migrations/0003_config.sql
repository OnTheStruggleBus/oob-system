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

-- Retiring a unit never deletes it: terminal IDs are never reused and the
-- audit trail references them. NULL means in service.
ALTER TABLE terminal ADD COLUMN retired_utc TEXT
    CHECK (retired_utc IS NULL OR length(retired_utc) > 0);

-- Precious. oob-mux's per-table PSI version counters and last-emitted
-- section bytes (#17 TableVersions::snapshot), so a restart continues the
-- mod-32 version count instead of re-emitting a changed table at version 0.
-- kind is the table kind's name as the daemon (#29) writes it. Rows are
-- upserted and never pruned: a retired table keeps its version so a re-add
-- continues the count, and the set is bounded by the number of table kinds.
CREATE TABLE psi_table_version (
    kind        TEXT    PRIMARY KEY CHECK (length(kind) > 0),
    version     INTEGER NOT NULL CHECK (version BETWEEN 0 AND 31),
    bytes       BLOB    NOT NULL,
    retired     INTEGER NOT NULL CHECK (retired IN (0, 1)),
    updated_utc TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
