CREATE TABLE IF NOT EXISTS escrows (
    id                 TEXT PRIMARY KEY,
    buyer_id           TEXT    NOT NULL,
    seller_id          TEXT    NOT NULL,
    amount_sats        INTEGER NOT NULL CHECK (amount_sats > 0),
    description        TEXT    NOT NULL,
    payment_hash       TEXT UNIQUE,
    invoice            TEXT,
    invoice_expires_at INTEGER,
    state              TEXT    NOT NULL,
    deadline_at        INTEGER NOT NULL,
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_log (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    escrow_id  TEXT    NOT NULL REFERENCES escrows(id),
    actor      TEXT    NOT NULL,
    action     TEXT    NOT NULL,
    from_state TEXT    NOT NULL,
    to_state   TEXT    NOT NULL,
    reason     TEXT    NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS audit_log_escrow ON audit_log(escrow_id, seq);

CREATE TABLE IF NOT EXISTS node_events (
    seq                INTEGER PRIMARY KEY AUTOINCREMENT,
    escrow_id          TEXT    NOT NULL REFERENCES escrows(id),
    node_invoice_state TEXT    NOT NULL,
    raw_event_hash     TEXT    NOT NULL,
    received_at        INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS node_events_escrow ON node_events(escrow_id, seq);
