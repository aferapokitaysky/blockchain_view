ALTER TABLE observed_transfers
    DROP CONSTRAINT IF EXISTS observed_transfers_transfer_kind_check;
ALTER TABLE observed_transfers
    DROP CONSTRAINT IF EXISTS observed_transfers_token_contract_address_check;

ALTER TABLE observed_transfers
    ADD CONSTRAINT observed_transfers_transfer_kind_check CHECK (
        transfer_kind IN (
            'bitcoin', 'evm_native', 'evm_internal', 'erc20', 'solana_native', 'solana_spl'
        )
    );

ALTER TABLE observed_transfers
    ADD CONSTRAINT observed_transfers_token_contract_address_check CHECK (
        token_contract_address IS NULL
        OR token_contract_address ~ '^0x[a-f0-9]{40}$'
        OR (
            transfer_kind = 'solana_spl'
            AND token_contract_address ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$'
        )
    ),
    ADD CONSTRAINT observed_transfers_solana_spl_mint_required CHECK (
        transfer_kind <> 'solana_spl' OR token_contract_address IS NOT NULL
    );

CREATE TABLE solana_sync_cursors (
    tracked_address_id UUID PRIMARY KEY REFERENCES tracked_addresses(id) ON DELETE CASCADE,
    newest_scanned_signature TEXT,
    newest_pending_before_signature TEXT,
    newest_pending_head_signature TEXT,
    oldest_scanned_signature TEXT,
    backfill_exhausted BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
