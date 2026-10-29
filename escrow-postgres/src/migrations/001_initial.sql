CREATE TABLE escrows (
    id                 TEXT PRIMARY KEY,
    -- NULL for an open offer that nobody has accepted yet.
    buyer_id           TEXT,
    seller_id          TEXT    NOT NULL,
    amount_sats        BIGINT  NOT NULL CHECK (amount_sats > 0),
    description        TEXT    NOT NULL,
    payment_hash       TEXT UNIQUE,
    invoice            TEXT,
    invoice_expires_at BIGINT,
    state              TEXT    NOT NULL,
    deadline_at        BIGINT  NOT NULL,
    created_at         BIGINT  NOT NULL,
    updated_at         BIGINT  NOT NULL
);
CREATE INDEX escrows_buyer ON escrows(buyer_id, created_at);
CREATE INDEX escrows_seller ON escrows(seller_id, created_at);

CREATE TABLE audit_log (
    seq        BIGSERIAL PRIMARY KEY,
    escrow_id  TEXT   NOT NULL REFERENCES escrows(id),
    actor      TEXT   NOT NULL,
    action     TEXT   NOT NULL,
    from_state TEXT   NOT NULL,
    to_state   TEXT   NOT NULL,
    reason     TEXT   NOT NULL,
    created_at BIGINT NOT NULL
);
CREATE INDEX audit_log_escrow ON audit_log(escrow_id, seq);

CREATE TABLE users (
    id         TEXT PRIMARY KEY,
    role       TEXT   NOT NULL,
    token_hash TEXT   NOT NULL UNIQUE,
    created_at BIGINT NOT NULL
);

CREATE TABLE node_events (
    seq                BIGSERIAL PRIMARY KEY,
    escrow_id          TEXT   NOT NULL REFERENCES escrows(id),
    node_invoice_state TEXT   NOT NULL,
    raw_event_hash     TEXT   NOT NULL,
    received_at        BIGINT NOT NULL
);
CREATE INDEX node_events_escrow ON node_events(escrow_id, seq);
