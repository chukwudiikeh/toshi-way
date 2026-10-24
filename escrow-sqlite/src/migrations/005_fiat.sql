-- An optional fiat value, taken once when the offer is made and never changed: what the sum was
-- worth in a currency then. Display only. Amounts are integers in the currency's minor unit and
-- the rate is minor units per bitcoin, so no floating point is stored. Existing escrows have none.
ALTER TABLE escrows ADD COLUMN fiat_currency     TEXT;
ALTER TABLE escrows ADD COLUMN fiat_amount_minor INTEGER;
ALTER TABLE escrows ADD COLUMN fiat_rate_minor   INTEGER;
ALTER TABLE escrows ADD COLUMN fiat_quoted_at    INTEGER;
ALTER TABLE escrows ADD COLUMN fiat_source       TEXT;
