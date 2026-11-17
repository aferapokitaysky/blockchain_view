ALTER TABLE tracked_addresses
    DROP CONSTRAINT IF EXISTS tracked_addresses_network_check;
ALTER TABLE tracked_addresses
    ADD CONSTRAINT tracked_addresses_network_check CHECK (
        network IN ('ethereum', 'bitcoin', 'base', 'arbitrum', 'solana', 'tron', 'xrpl')
        OR CASE
            WHEN network ~ '^eip155:[1-9][0-9]{0,19}$'
                THEN split_part(network, ':', 2)::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END
    );

ALTER TABLE observed_transfers
    DROP CONSTRAINT IF EXISTS observed_transfers_transfer_kind_check,
    DROP CONSTRAINT IF EXISTS observed_transfers_token_contract_address_check,
    DROP CONSTRAINT IF EXISTS observed_transfers_solana_spl_mint_required,
    DROP CONSTRAINT IF EXISTS observed_transfers_token_transfer_mint_required;

ALTER TABLE observed_transfers
    ADD CONSTRAINT observed_transfers_transfer_kind_check CHECK (
        transfer_kind IN (
            'bitcoin', 'evm_native', 'evm_internal', 'erc20',
            'solana_native', 'solana_spl', 'tron_native', 'trc20', 'xrpl_native'
        )
    ),
    ADD CONSTRAINT observed_transfers_token_contract_address_check CHECK (
        token_contract_address IS NULL
        OR (transfer_kind = 'erc20' AND token_contract_address ~ '^0x[a-f0-9]{40}$')
        OR (transfer_kind IN ('solana_spl', 'trc20') AND token_contract_address ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$')
    ),
    ADD CONSTRAINT observed_transfers_token_transfer_mint_required CHECK (
        transfer_kind NOT IN ('solana_spl', 'trc20') OR token_contract_address IS NOT NULL
    );

CREATE TABLE xrpl_sync_cursors (
    tracked_address_id UUID PRIMARY KEY REFERENCES tracked_addresses(id) ON DELETE CASCADE,
    head_transaction_hash TEXT,
    head_pending_marker JSONB,
    backfill_marker JSONB,
    backfill_exhausted BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
