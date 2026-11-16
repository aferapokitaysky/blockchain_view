# Architecture

## Current implementation

The repository contains a React, TypeScript, Vite, and Tailwind frontend plus a local Rust/Axum API.

- The wallet form detects Bitcoin address syntax, asks the user to choose among ambiguous EVM networks, and saves investigations and tracked addresses through the local API.
- The overview graph, balances, node labels, and risk panel still use static demo data. The recent-transactions table reads persisted observations for saved wallets, with provider, tracked address, amount, status, and time. The saved-wallet section has a separate on-demand transfer-links view built from persisted EVM sender/recipient fields. It marks counterparties that are not tracked. Bitcoin address-level input/output observations do not identify counterparties and are excluded from this graph.
- PostgreSQL is the persistent store. Migrations create `investigations`, `tracked_addresses`, `risk_signals`, and `observed_transfers`.
- Redis is connected to the API readiness check and stores the continuous history sync queue for the optional worker.
- The unauthenticated API is intended for local development only.

## API and storage flow

```text
Wallet form / local API request
               |
          Rust + Axum
        /      |       \
 PostgreSQL  Redis Streams  Configured chain providers
   records  sync queue     on-demand/background history
```

The API creates and lists investigations and tracked addresses. A manual `POST` sync request retrieves Bitcoin history from the configured Esplora-compatible service or supported EVM history from Blockscout, normalizes address activity, and upserts observed transfers in PostgreSQL. EVM rows retain event identity and direct sender/recipient addresses. A separate `GET` returns up to 500 most recent saved transfer rows for that tracked address.

For each transaction, records preserve the configured Esplora host, observation time, transaction hash, confirmation state, block metadata, and address-level input/output totals. `spent_amount` is the value in inputs attributed to the tracked address; `received_amount` is the value of outputs to it. Amounts are stored as integer satoshis and serialized as strings to avoid precision loss. These are independent input/output totals, not a net balance change.

The risk-signal API stores analyst-entered evidence scoped to an investigation and tracked address. Each operation verifies both path IDs against the same address record. Signals retain their key, category, severity, source name and reference, observation time, structured details, and creation time. Request bodies are limited to 16 KiB and `details` to 8 KiB. These records are not automatically sourced or verified and do not change the frontend's provisional score.

OFAC SDN screening uses an XML snapshot imported into PostgreSQL by a separate command. The query endpoint returns exact-address matches, snapshot publication/import metadata, or an explicit unsupported/not-imported status. Bitcoin maps to OFAC currency `XBT`; Ethereum maps to `ETH`. Base and Arbitrum return `network_not_covered`. Results are not automatically converted into analyst signals or the demo score. See [OFAC screening setup](ofac-screening.md).

The optional worker scans tracked addresses every 30 seconds in batches, enqueues deduplicated jobs in a bounded Redis Stream, and runs the same provider-backed sync code as the manual endpoint. See [continuous sync](continuous-sync.md).

## Provider scope and limits

- Bitcoin and selected EVM history can be fetched on demand and by the optional scheduled worker.
- Sync requests the initial Esplora history page, which may include unconfirmed mempool entries, then follows confirmed history for at most 20 pages total. This is up to 500 confirmed transactions, plus any mempool entries in the initial response.
- The transfers endpoint returns at most 500 recent stored records. A sync can fetch more than that when the first page includes mempool entries, so the read endpoint is a bounded recent view.
- Bitcoin history sync uses Esplora. EVM history uses Blockscout for Ethereum (1), Base (8453), and Arbitrum One (42161), fetching normal native transactions, successful internal native transfers, and ERC-20 events. Each stream is capped at 10 pages of 50 rows; `may_have_more_history` reports when a provider cursor remains at the cap. Both manual requests and the background worker use these bounds. This is not a complete index or identity attribution; other EVM networks and token standards are excluded.
- The API supports create, list, read, replace, and delete operations for analyst-entered signals. Exact-address screening uses a locally imported OFAC SDN snapshot for Bitcoin and Ethereum. There is no background refresh or indirect-exposure screening; the snapshot publication date is returned to make its age visible. Results do not alter the demo exposure score.

## Network identity and address handling

An address string alone is not a reliable cross-chain identity. Every tracked address and transaction must include a network identifier. Bitcoin addresses are checked against Bitcoin mainnet format. EVM-format addresses may be valid on several networks, so the UI asks the user to select the intended chain; format alone cannot distinguish Ethereum, Base, and Arbitrum. The API normalizes EVM hex addresses but does not infer their network.

## Reliability and safety

- Validate request payloads and keep investigation/address relationships scoped in each query.
- Apply bounded provider request timeouts and idempotent upserts by tracked address and transaction hash.
- Surface provider and observation time so analysts can inspect provenance and freshness.
- Keep the API bound to loopback for local use. It has no authentication and must not be exposed to an untrusted network.
- Keep demo data visibly separate from provider-backed observations: the overview graph remains illustrative, while the separate transfer-links view uses persisted EVM sender/recipient fields.
