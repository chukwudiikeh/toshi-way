-- API keys let a business's own software call the API without holding the account token. A key can
-- only do what its scopes say, and can be revoked. Only the SHA-256 of the key is stored.
CREATE TABLE api_keys (
    id           TEXT PRIMARY KEY,
    user_id      TEXT   NOT NULL REFERENCES users(id),
    name         TEXT   NOT NULL,
    key_hash     TEXT   NOT NULL UNIQUE,
    last_chars   TEXT   NOT NULL,
    scopes       TEXT   NOT NULL,
    created_at   BIGINT NOT NULL,
    last_used_at BIGINT,
    revoked_at   BIGINT
);
CREATE INDEX api_keys_by_user ON api_keys (user_id, created_at DESC);
