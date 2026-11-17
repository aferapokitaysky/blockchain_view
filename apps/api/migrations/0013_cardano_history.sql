ALTER TABLE tracked_addresses
    DROP CONSTRAINT IF EXISTS tracked_addresses_network_check;
ALTER TABLE tracked_addresses
    ADD CONSTRAINT tracked_addresses_network_check CHECK (
        network IN ('ethereum', 'bitcoin', 'base', 'arbitrum', 'solana', 'tron', 'xrpl', 'ton', 'cardano')
        OR CASE
            WHEN network ~ '^eip155:[1-9][0-9]{0,19}$'
                THEN split_part(network, ':', 2)::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END
    );

ALTER TABLE observed_transfers
    DROP CONSTRAINT IF EXISTS observed_transfers_transfer_kind_check,
    DROP CONSTRAINT IF EXISTS observed_transfers_token_contract_address_check,
    DROP CONSTRAINT IF EXISTS observed_transfers_token_transfer_mint_required;

ALTER TABLE observed_transfers
    ADD CONSTRAINT observed_transfers_transfer_kind_check CHECK (
        transfer_kind IN (
            'bitcoin', 'evm_native', 'evm_internal', 'erc20',
            'solana_native', 'solana_spl', 'tron_native', 'trc20', 'xrpl_native',
            'ton_native', 'ton_jetton', 'cardano_native', 'cardano_asset'
        )
    ),
    ADD CONSTRAINT observed_transfers_token_contract_address_check CHECK (
        token_contract_address IS NULL
        OR (transfer_kind = 'erc20' AND token_contract_address ~ '^0x[a-f0-9]{40}$')
        OR (transfer_kind IN ('solana_spl', 'trc20') AND token_contract_address ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$')
        OR (transfer_kind = 'ton_jetton' AND token_contract_address ~ '^-?[0-9]+:[0-9a-f]{64}$')
        OR (transfer_kind = 'cardano_asset' AND token_contract_address ~ '^[a-f0-9]{56}[a-f0-9]{0,64}$')
    ),
    ADD CONSTRAINT observed_transfers_token_transfer_mint_required CHECK (
        transfer_kind NOT IN ('solana_spl', 'trc20', 'ton_jetton', 'cardano_asset')
        OR token_contract_address IS NOT NULL
    );

CREATE TABLE cardano_sync_cursors (
    tracked_address_id UUID PRIMARY KEY REFERENCES tracked_addresses(id) ON DELETE CASCADE,
    newest_tx_hash TEXT,
    next_backfill_page INTEGER NOT NULL DEFAULT 1 CHECK (next_backfill_page >= 1),
    backfill_exhausted BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (newest_tx_hash IS NULL OR newest_tx_hash ~ '^[a-f0-9]{64}$')
);

CREATE TABLE observed_relationships (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tracked_address_id UUID NOT NULL REFERENCES tracked_addresses(id) ON DELETE CASCADE,
    network TEXT NOT NULL CHECK (network = 'cardano'),
    transaction_hash TEXT NOT NULL CHECK (transaction_hash ~ '^[a-f0-9]{64}$'),
    counterparty_address TEXT NOT NULL CHECK (length(counterparty_address) BETWEEN 20 AND 128),
    relationship_type TEXT NOT NULL CHECK (relationship_type = 'same_transaction'),
    participant_role TEXT NOT NULL CHECK (participant_role IN ('input', 'output', 'input_and_output')),
    block_height BIGINT CHECK (block_height >= 0),
    block_time TIMESTAMPTZ,
    confirmed BOOLEAN NOT NULL,
    provider TEXT NOT NULL,
    source_host TEXT NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tracked_address_id, transaction_hash, counterparty_address, relationship_type)
);

CREATE INDEX observed_relationships_address_time_idx
    ON observed_relationships (tracked_address_id, block_time DESC NULLS LAST, observed_at DESC);

CREATE TABLE cardano_asset_metadata (
    asset_id TEXT PRIMARY KEY CHECK (asset_id ~ '^[a-f0-9]{56}[a-f0-9]{0,64}$'),
    asset_name TEXT,
    decimals SMALLINT CHECK (decimals BETWEEN 0 AND 255),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE investigation_graph_snapshots
    ADD COLUMN source_relationship_ids UUID[] NOT NULL DEFAULT '{}';
ALTER TABLE investigation_graph_snapshots
    DROP CONSTRAINT IF EXISTS investigation_graph_snapshots_source_transfer_ids_check;
ALTER TABLE investigation_graph_snapshots
    ADD CONSTRAINT investigation_graph_snapshots_source_evidence_ids_check CHECK (
        cardinality(source_transfer_ids) + cardinality(source_relationship_ids) BETWEEN 1 AND 10000
    );
