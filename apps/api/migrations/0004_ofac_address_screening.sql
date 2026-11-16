CREATE TABLE sanctions_list_snapshots (
    source_code TEXT PRIMARY KEY,
    source_name TEXT NOT NULL,
    published_at TEXT NOT NULL,
    imported_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    address_count INTEGER NOT NULL CHECK (address_count > 0)
);

CREATE TABLE sanctions_addresses (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    source_code TEXT NOT NULL REFERENCES sanctions_list_snapshots(source_code) ON DELETE CASCADE,
    entity_uid TEXT NOT NULL,
    entity_name TEXT NOT NULL,
    programs JSONB NOT NULL DEFAULT '[]'::jsonb,
    currency_code TEXT NOT NULL,
    address TEXT NOT NULL,
    address_normalized TEXT NOT NULL,
    UNIQUE (source_code, currency_code, address_normalized, entity_uid)
);

CREATE INDEX sanctions_addresses_exact_lookup_idx
    ON sanctions_addresses (source_code, currency_code, address_normalized);
