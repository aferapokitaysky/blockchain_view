CREATE TABLE solana_helius_sync_cursors (
    tracked_address_id UUID PRIMARY KEY REFERENCES tracked_addresses(id) ON DELETE CASCADE,
    head_slot BIGINT NOT NULL DEFAULT 0 CHECK (head_slot >= 0),
    backfill_token TEXT,
    backfill_exhausted BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
