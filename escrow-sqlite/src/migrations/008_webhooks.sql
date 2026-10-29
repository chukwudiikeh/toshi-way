-- Webhooks: a business gives an address to be told about changes to its escrows. Deliveries are
-- driven by the audit log: `last_seq` is the last audit_log seq already delivered to this endpoint,
-- so a restart loses nothing and events go out in order. The signing secret is stored sealed.
CREATE TABLE webhooks (
    id              TEXT PRIMARY KEY,
    user_id         TEXT    NOT NULL REFERENCES users(id),
    url             TEXT    NOT NULL,
    signing_key_enc      BLOB    NOT NULL,
    created_at      INTEGER NOT NULL,
    last_seq        INTEGER NOT NULL,
    attempts        INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER NOT NULL,
    failing_since   INTEGER,
    disabled_at     INTEGER,
    last_attempt_at INTEGER,
    last_status     TEXT
);
CREATE INDEX webhooks_by_user ON webhooks (user_id, created_at);
