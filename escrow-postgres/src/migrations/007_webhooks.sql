-- Webhooks: a business gives an address to be told about changes to its escrows. Deliveries are
-- driven by the audit log: `last_seq` is the last audit_log seq already delivered to this endpoint,
-- so a restart loses nothing and events go out in order. The signing secret is stored sealed.
CREATE TABLE webhooks (
    id              TEXT PRIMARY KEY,
    user_id         TEXT    NOT NULL REFERENCES users(id),
    url             TEXT    NOT NULL,
    signing_key_enc      BYTEA   NOT NULL,
    created_at      BIGINT NOT NULL,
    last_seq        BIGINT NOT NULL,
    attempts        BIGINT NOT NULL DEFAULT 0,
    next_attempt_at BIGINT NOT NULL,
    failing_since   BIGINT,
    disabled_at     BIGINT,
    last_attempt_at BIGINT,
    last_status     TEXT
);
CREATE INDEX webhooks_by_user ON webhooks (user_id, created_at);
