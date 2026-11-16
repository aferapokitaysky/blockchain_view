ALTER TABLE tracked_addresses
    DROP CONSTRAINT IF EXISTS tracked_addresses_network_check;

ALTER TABLE tracked_addresses
    ADD CONSTRAINT tracked_addresses_network_check CHECK (
        network IN ('ethereum', 'bitcoin', 'base', 'arbitrum')
        OR CASE
            WHEN network ~ '^eip155:[1-9][0-9]{0,19}$'
                THEN split_part(network, ':', 2)::NUMERIC <= 18446744073709551615
            ELSE FALSE
        END
    );
