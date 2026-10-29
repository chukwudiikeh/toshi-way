-- A business's own order number and a link back to its checkout, both optional and set when the
-- offer is made. Existing escrows have neither.
ALTER TABLE escrows ADD COLUMN reference  TEXT;
ALTER TABLE escrows ADD COLUMN return_url TEXT;
