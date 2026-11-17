ALTER TABLE tracked_addresses
    DROP CONSTRAINT IF EXISTS tracked_addresses_network_check;
ALTER TABLE tracked_addresses
    ADD CONSTRAINT tracked_addresses_network_check CHECK (
        network IN ('ethereum', 'bitcoin', 'base', 'arbitrum', 'solana', 'tron', 'xrpl', 'ton', 'cardano', 'aptos', 'sui')
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
            'ton_native', 'ton_jetton', 'cardano_native', 'cardano_asset',
            'aptos_fungible', 'sui_balance_change'
        )
    ),
    ADD CONSTRAINT observed_transfers_token_contract_address_check CHECK (
        token_contract_address IS NULL
        OR (transfer_kind = 'erc20' AND token_contract_address ~ '^0x[a-f0-9]{40}$')
        OR (transfer_kind IN ('solana_spl', 'trc20') AND token_contract_address ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$')
        OR (transfer_kind = 'ton_jetton' AND token_contract_address ~ '^-?[0-9]+:[0-9a-f]{64}$')
        OR (transfer_kind = 'cardano_asset' AND token_contract_address ~ '^[a-f0-9]{56}[a-f0-9]{0,64}$')
        OR (transfer_kind = 'aptos_fungible' AND token_contract_address ~ '^0x[a-f0-9]{1,64}::[A-Za-z_][A-Za-z0-9_]*::[A-Za-z_][A-Za-z0-9_]*$')
    ),
    ADD CONSTRAINT observed_transfers_token_transfer_mint_required CHECK (
        transfer_kind NOT IN ('solana_spl', 'trc20', 'ton_jetton', 'cardano_asset', 'aptos_fungible')
        OR token_contract_address IS NOT NULL
    );

ALTER TABLE observed_relationships
    DROP CONSTRAINT IF EXISTS observed_relationships_network_check,
    DROP CONSTRAINT IF EXISTS observed_relationships_transaction_hash_check,
    DROP CONSTRAINT IF EXISTS observed_relationships_counterparty_address_check;

ALTER TABLE observed_relationships
    ADD CONSTRAINT observed_relationships_network_check CHECK (network IN ('cardano', 'sui')),
    ADD CONSTRAINT observed_relationships_transaction_hash_check CHECK (
        (network = 'cardano' AND transaction_hash ~ '^[a-f0-9]{64}$')
        OR (network = 'sui' AND transaction_hash ~ '^[1-9A-HJ-NP-Za-km-z]{40,50}$')
    ),
    ADD CONSTRAINT observed_relationships_counterparty_address_check CHECK (
        length(counterparty_address) BETWEEN 3 AND 128
    );

CREATE TABLE sui_sync_cursors (
    tracked_address_id UUID PRIMARY KEY REFERENCES tracked_addresses(id) ON DELETE CASCADE,
    head_cursor TEXT,
    backfill_cursor TEXT,
    backfill_exhausted BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (head_cursor IS NULL OR length(head_cursor) <= 8192),
    CHECK (backfill_cursor IS NULL OR length(backfill_cursor) <= 8192)
);
