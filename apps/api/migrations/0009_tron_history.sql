ALTER TABLE observed_transfers
    DROP CONSTRAINT IF EXISTS observed_transfers_transfer_kind_check;
ALTER TABLE observed_transfers
    DROP CONSTRAINT IF EXISTS observed_transfers_token_contract_address_check;

ALTER TABLE observed_transfers
    ADD CONSTRAINT observed_transfers_transfer_kind_check CHECK (
        transfer_kind IN (
            'bitcoin', 'evm_native', 'evm_internal', 'erc20',
            'solana_native', 'solana_spl', 'tron_native', 'trc20'
        )
    );

ALTER TABLE observed_transfers
    ADD CONSTRAINT observed_transfers_token_contract_address_check CHECK (
        token_contract_address IS NULL
        OR (transfer_kind = 'erc20' AND token_contract_address ~ '^0x[a-f0-9]{40}$')
        OR (
            transfer_kind IN ('solana_spl', 'trc20')
            AND token_contract_address ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$'
        )
    ),
    ADD CONSTRAINT observed_transfers_token_transfer_mint_required CHECK (
        transfer_kind NOT IN ('solana_spl', 'trc20') OR token_contract_address IS NOT NULL
    );

CREATE TABLE tron_sync_cursors (
    tracked_address_id UUID NOT NULL REFERENCES tracked_addresses(id) ON DELETE CASCADE,
    stream TEXT NOT NULL CHECK (stream IN ('native', 'trc20')),
    head_transaction_id TEXT,
    pending_head_transaction_id TEXT,
    pending_fingerprint TEXT,
    backfill_fingerprint TEXT,
    backfill_exhausted BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tracked_address_id, stream),
    CHECK ((pending_head_transaction_id IS NULL) = (pending_fingerprint IS NULL))
);
