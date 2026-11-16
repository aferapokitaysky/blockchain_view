ALTER TABLE observed_transfers
    DROP CONSTRAINT IF EXISTS observed_transfers_tracked_address_id_transaction_hash_key;

ALTER TABLE observed_transfers
    ADD COLUMN event_index BIGINT NOT NULL DEFAULT -1 CHECK (event_index >= -1),
    ADD COLUMN transfer_kind TEXT NOT NULL DEFAULT 'bitcoin'
        CHECK (transfer_kind IN ('bitcoin', 'evm_native', 'evm_internal', 'erc20')),
    ADD COLUMN token_contract_address TEXT
        CHECK (token_contract_address IS NULL OR token_contract_address ~ '^0x[a-f0-9]{40}$'),
    ADD COLUMN token_decimals SMALLINT
        CHECK (token_decimals IS NULL OR token_decimals BETWEEN 0 AND 255),
    ADD COLUMN source_host TEXT NOT NULL DEFAULT 'unknown',
    ADD COLUMN from_address TEXT,
    ADD COLUMN to_address TEXT;

UPDATE observed_transfers SET source_host = provider WHERE source_host = 'unknown';

ALTER TABLE observed_transfers
    ADD CONSTRAINT observed_transfers_event_identity_key
        UNIQUE (tracked_address_id, transaction_hash, transfer_kind, event_index);
