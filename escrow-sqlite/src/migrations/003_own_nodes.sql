-- Sellers can bring their own Lightning node. Every escrow so far was served by the service's
-- shared node, which is what the default records. The macaroon is stored encrypted (nonce +
-- ciphertext) by the application; the database never sees it in the clear.
ALTER TABLE escrows ADD COLUMN node TEXT NOT NULL DEFAULT 'shared' CHECK (node IN ('own', 'shared'));

CREATE TABLE node_connections (
    user_id      TEXT PRIMARY KEY REFERENCES users(id),
    address      TEXT    NOT NULL,
    tls_cert     TEXT    NOT NULL,
    macaroon_enc BLOB    NOT NULL,
    updated_at   INTEGER NOT NULL
);
