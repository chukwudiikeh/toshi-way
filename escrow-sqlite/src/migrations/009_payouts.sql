-- Payouts of the sats the operator owes sellers on the shared node, and conversions to and from local
-- currency through a ramp provider. A seller's balance is never stored: it is worked out from their
-- settled shared-node escrows less these withdrawals, so it cannot drift from the escrows.
CREATE TABLE withdrawals (
    id             TEXT PRIMARY KEY,
    user_id        TEXT    NOT NULL,
    kind           TEXT    NOT NULL,
    amount_sats    INTEGER NOT NULL CHECK (amount_sats > 0),
    fee_limit_sats INTEGER NOT NULL,
    fee_sats       INTEGER,
    invoice        TEXT    NOT NULL,
    payment_hash   TEXT    NOT NULL,
    state          TEXT    NOT NULL,
    failure        TEXT,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);
CREATE INDEX withdrawals_by_user ON withdrawals (user_id, created_at);
CREATE INDEX withdrawals_pending ON withdrawals (state) WHERE state = 'pending';
-- An invoice is paid once: only a failed attempt leaves it free to be tried again.
CREATE UNIQUE INDEX withdrawals_live_hash ON withdrawals (payment_hash) WHERE state <> 'failed';

CREATE TABLE ramp_orders (
    id                     TEXT PRIMARY KEY,
    user_id                TEXT    NOT NULL,
    direction              TEXT    NOT NULL,
    escrow_id              TEXT REFERENCES escrows(id),
    withdrawal_id          TEXT,
    amount_sats            INTEGER NOT NULL,
    currency               TEXT    NOT NULL,
    fiat_minor             INTEGER NOT NULL,
    fee_minor              INTEGER NOT NULL,
    rate_minor_per_btc     INTEGER NOT NULL,
    bank_code              TEXT    NOT NULL,
    bank_name              TEXT    NOT NULL,
    account_number         TEXT    NOT NULL,
    account_name           TEXT    NOT NULL,
    deposit_bank_name      TEXT,
    deposit_account_number TEXT,
    deposit_account_name   TEXT,
    deposit_reference      TEXT,
    deposit_expires_at     INTEGER,
    provider               TEXT    NOT NULL,
    fiat_reference         TEXT,
    state                  TEXT    NOT NULL,
    note                   TEXT,
    created_at             INTEGER NOT NULL,
    updated_at             INTEGER NOT NULL
);
CREATE INDEX ramp_orders_by_user ON ramp_orders (user_id, created_at);
CREATE INDEX ramp_orders_by_escrow ON ramp_orders (escrow_id, created_at);
-- One order in progress per escrow.
CREATE UNIQUE INDEX ramp_orders_live_escrow ON ramp_orders (escrow_id)
    WHERE escrow_id IS NOT NULL AND state NOT IN ('complete', 'refunded', 'expired', 'paid_out', 'failed');
