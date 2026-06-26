CREATE SEQUENCE IF NOT EXISTS payment_order_no_seq START WITH 10000001;

ALTER TABLE payment
    ADD COLUMN IF NOT EXISTS order_no BIGINT;

UPDATE payment
SET order_no = nextval('payment_order_no_seq')
WHERE order_no IS NULL;

ALTER TABLE payment
    ALTER COLUMN order_no SET DEFAULT nextval('payment_order_no_seq'),
    ALTER COLUMN order_no SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_payment_order_no ON payment(order_no);
