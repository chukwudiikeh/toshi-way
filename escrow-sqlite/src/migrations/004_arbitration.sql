-- Optional arbitration. An escrow may name an arbiter; the arbiter's public key is recorded on the
-- escrow when it is created (so it cannot be swapped later), the buyer's release secret encrypted
-- to that key is stored as an opaque blob the server cannot read, and the arbiter's confirmation
-- time is kept. Existing escrows have no arbiter.
ALTER TABLE escrows ADD COLUMN arbiter_id           TEXT;
ALTER TABLE escrows ADD COLUMN arbiter_key          TEXT;
ALTER TABLE escrows ADD COLUMN arbiter_sealed       TEXT;
ALTER TABLE escrows ADD COLUMN arbiter_confirmed_at INTEGER;
CREATE INDEX escrows_arbiter ON escrows(arbiter_id, created_at);

CREATE TABLE arbiter_keys (
    user_id    TEXT PRIMARY KEY REFERENCES users(id),
    public_key TEXT    NOT NULL,
    updated_at INTEGER NOT NULL
);
