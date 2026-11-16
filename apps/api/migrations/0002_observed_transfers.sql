CREATE TABLE observed_transfers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tracked_address_id UUID NOT NULL REFERENCES tracked_addresses(id) ON DELETE CASCADE,
    network TEXT NOT NULL,
    transaction_hash TEXT NOT NULL,
    direction TEXT NOT NULL CHECK (direction IN ('inbound', 'outbound', 'both')),
    received_amount NUMERIC(78, 0) NOT NULL DEFAULT 0 CHECK (received_amount >= 0),
    spent_amount NUMERIC(78, 0) NOT NULL DEFAULT 0 CHECK (spent_amount >= 0),
    asset TEXT NOT NULL,
    block_height BIGINT,
    block_time TIMESTAMPTZ,
    confirmed BOOLEAN NOT NULL,
    provider TEXT NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tracked_address_id, transaction_hash),
    CHECK (block_height IS NULL OR block_height >= 0)
);

CREATE INDEX observed_transfers_address_time_idx
    ON observed_transfers (tracked_address_id, block_time DESC NULLS LAST, observed_at DESC);
