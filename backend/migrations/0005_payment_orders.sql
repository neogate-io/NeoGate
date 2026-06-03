CREATE TABLE payment (
    id UUID PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    wallet_id BIGINT NOT NULL REFERENCES wallet(id),
    provider TEXT NOT NULL,
    provider_order_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'paid', 'failed', 'canceled', 'expired')),
    currency TEXT NOT NULL,
    amount_micro_usd BIGINT NOT NULL CHECK (amount_micro_usd > 0),
    payable_amount_minor BIGINT NOT NULL CHECK (payable_amount_minor > 0),
    checkout_url TEXT,
    return_url TEXT,
    notify_payload JSONB,
    paid_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_payment_provider_order
    ON payment(provider, provider_order_id)
    WHERE provider_order_id IS NOT NULL;

CREATE INDEX idx_payment_user_created ON payment(user_id, created_at DESC);
CREATE INDEX idx_payment_pending ON payment(provider, created_at ASC)
    WHERE status = 'pending';

CREATE TABLE payment_event (
    id BIGSERIAL PRIMARY KEY,
    payment_id UUID REFERENCES payment(id) ON DELETE SET NULL,
    provider TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_payment_event_payment_created ON payment_event(payment_id, created_at DESC);
