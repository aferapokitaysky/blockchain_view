ALTER TABLE cardano_sync_cursors
    ADD COLUMN newest_block_height BIGINT CHECK (newest_block_height >= 0),
    ADD COLUMN newest_tx_index INTEGER CHECK (newest_tx_index >= 0),
    ADD CONSTRAINT cardano_sync_cursors_head_position_check CHECK (
        (newest_block_height IS NULL) = (newest_tx_index IS NULL)
    );
