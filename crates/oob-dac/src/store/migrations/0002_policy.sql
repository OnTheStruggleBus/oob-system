-- 0002_policy: the policy layer's two tables (issue #23, r6 F6-10, ADR-003 QA-7).
--
-- Both have an entry in store/classify.rs TABLES in the same commit.
-- Forward-only: this file never changes once released.

-- DerivedObserved. Short-lived confirmation tokens, stored only as the
-- SHA-256 of the token (the token itself is shown once, at issue). Losing
-- them on a restore or rebuild is correct: an unspent token is worth
-- nothing after a restart, and StorePolicy::new purges them anyway
-- (issued_ms and expires_ms are monotonic-clock milliseconds, which do not
-- survive a restart).
CREATE TABLE confirmation_token (
    token_hash TEXT PRIMARY KEY CHECK (length(token_hash) = 64),
    operation  TEXT NOT NULL,
    -- 'terminal:<id>', 'plan:<sha256 hex>' or 'install:<install id>'.
    target     TEXT NOT NULL,
    issued_by  TEXT NOT NULL,
    issued_ms  INTEGER NOT NULL,
    expires_ms INTEGER NOT NULL,
    used       INTEGER NOT NULL DEFAULT 0 CHECK (used IN (0, 1))
) STRICT;

-- Precious. Per-install owner settings the policy layer reads. The
-- sacrificial-only CDL lock is seeded ON so every fresh store starts
-- locked (ADR-003 section 5, Marshal QA-7). install_id names this
-- installation in the typed confirmation phrase "CDL LOCK OFF <install id>";
-- it is random per file and identifies no box.
CREATE TABLE install_setting (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

INSERT INTO install_setting (key, value) VALUES
    ('cdl_sacrificial_only', '1'),
    ('install_id', lower(hex(randomblob(8))));
