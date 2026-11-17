CREATE TABLE investigation_graph_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    investigation_id UUID NOT NULL REFERENCES investigations(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 120),
    focus_network TEXT,
    focus_address TEXT,
    max_depth SMALLINT NOT NULL CHECK (max_depth BETWEEN 1 AND 3),
    source_transfer_ids UUID[] NOT NULL CHECK (cardinality(source_transfer_ids) BETWEEN 1 AND 10000),
    graph JSONB NOT NULL CHECK (jsonb_typeof(graph) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((focus_network IS NULL) = (focus_address IS NULL))
);

CREATE INDEX investigation_graph_snapshots_created_idx
    ON investigation_graph_snapshots (investigation_id, created_at DESC);
