-- An offer can be created without a named buyer; the first user to accept it becomes the buyer.
-- SQLite cannot drop a NOT NULL constraint in place, so the table is rebuilt.
CREATE TABLE escrows_new (
    id                 TEXT PRIMARY KEY,
    buyer_id           TEXT,
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

INSERT INTO escrows_new (id, buyer_id, seller_id, amount_sats, description, payment_hash, invoice,
                         invoice_expires_at, state, deadline_at, created_at, updated_at)
SELECT id, buyer_id, seller_id, amount_sats, description, payment_hash, invoice,
       invoice_expires_at, state, deadline_at, created_at, updated_at
FROM escrows;

DROP TABLE escrows;
ALTER TABLE escrows_new RENAME TO escrows;

CREATE INDEX escrows_buyer ON escrows(buyer_id, created_at);
CREATE INDEX escrows_seller ON escrows(seller_id, created_at);
