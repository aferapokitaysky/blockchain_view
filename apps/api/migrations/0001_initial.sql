CREATE TABLE investigations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 120),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE tracked_addresses (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    investigation_id UUID NOT NULL REFERENCES investigations(id) ON DELETE CASCADE,
    network TEXT NOT NULL CHECK (network IN ('ethereum', 'bitcoin', 'base', 'arbitrum')),
    address TEXT NOT NULL CHECK (length(address) BETWEEN 1 AND 128),
    label TEXT CHECK (label IS NULL OR length(btrim(label)) BETWEEN 1 AND 80),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (investigation_id, network, address)
);

CREATE INDEX tracked_addresses_investigation_created_idx
    ON tracked_addresses (investigation_id, created_at DESC);

CREATE TABLE risk_signals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tracked_address_id UUID NOT NULL REFERENCES tracked_addresses(id) ON DELETE CASCADE,
    signal_key TEXT NOT NULL,
    category TEXT NOT NULL CHECK (category IN ('sanctions', 'exposure', 'behavior', 'other')),
    severity TEXT NOT NULL CHECK (severity IN ('critical', 'high', 'medium', 'low', 'informational')),
    source_name TEXT NOT NULL CHECK (length(btrim(source_name)) BETWEEN 1 AND 120),
    source_reference TEXT,
    observed_at TIMESTAMPTZ NOT NULL,
    details JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX risk_signals_address_observed_idx
    ON risk_signals (tracked_address_id, observed_at DESC);
