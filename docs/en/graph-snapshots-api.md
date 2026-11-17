# Investigation graph snapshots API

The graph explorer uses saved observations from one investigation. EVM, TRON, Solana, XRPL, TON, and Aptos records with sender and recipient addresses can form directed edges. Cardano and Sui can add undirected same-transaction participant evidence; these links are not direct transfers and are excluded from directed path finding. A Sui link requires a complete set of exactly two equal-and-opposite owner balance changes for one coin type. This indicates matching net changes in one transaction, not a direct payment or continuation of the same funds. Bitcoin history remains available in the feed, but address-level UTXO records do not identify counterparties.

## Routes

`GET /api/v1/investigations/{investigation_id}/graph-snapshots` returns the latest 100 snapshots.

`POST /api/v1/investigations/{investigation_id}/graph-snapshots` stores a fixed graph view and its source observation IDs. The API checks that every source ID belongs to the same investigation.

```json
{
  "title": "TRON flow",
  "focus_network": "tron",
  "focus_address": "TUoHaVjx7n5xz8LwPRDckgFrDWhMhuSuJM",
  "max_depth": 2,
  "source_transfer_ids": ["00000000-0000-4000-8000-000000000001"],
  "graph": {
    "nodes": [],
    "edges": [],
    "truncated": false,
    "sourceTransferIds": []
  }
}
```

Titles are limited to 120 characters, depth to 1–3 hops, source observations to 10,000 IDs, and the graph payload to 1 MiB. Snapshots preserve the displayed graph even when new observations are later synced. They are evidence snapshots, not proof that funds moved continuously between hops. The API currently has no authentication and is intended for local development.
