# Tracefold

Open-source workspace for exploring how funds move between blockchain addresses.

> **Project status: early prototype.** The overview graph, wallet labels, and balances remain illustrative demo data. The recent-transactions table reads persisted observations for saved wallets and stays empty until history is synced through a configured provider. The risk panel loads analyst-entered signals for the selected saved address and may include an exact OFAC SDN match after an explicit local snapshot check; its versioned score is provisional, not a legal conclusion or complete compliance check. Manual sync is available in the saved-wallet panel. `docker compose up` also starts `sync-worker`, which periodically refreshes tracked Bitcoin and supported EVM addresses, targeting five minutes per address when provider capacity allows; this is bounded history polling, not real-time or full mempool monitoring. The investigation graph links saved sender/recipient events from EVM, TRON, Solana, and XRPL histories, explores directed paths up to three hops, and can save a fixed snapshot with its source observation IDs. Each hop is independently observed; a path does not establish that the same funds continued across hops. Bitcoin counterparties are not inferred. Exact-address screening against a locally imported OFAC SDN snapshot is available for Bitcoin and Ethereum. Do not use demo values for operational or compliance decisions.

**Documentation:** [Русский](README.ru.md) · [Українська](README.uk.md) · [English](README.md)

## What it is

Tracefold is being designed as an OSINT investigation tool. An analyst should be able to start from an address, follow transfers across supported networks, preserve the evidence behind each relationship, and review risk signals without treating an algorithmic score as proof.

The interface prototype includes a demo overview graph and an evidence-based, multichain transfer-links view with saved snapshots, alongside network filters, a persisted-observation table, and a versioned, explainable assessment for the selected saved wallet.

## Product principles

- Show the chain, address, transaction reference, source, and observation time for every finding.
- Keep observed transactions separate from analyst hypotheses and labels.
- Treat network detection as a candidate: an EVM-format address can exist on Ethereum, Base, Arbitrum, and other EVM chains.
- Describe a score as an exposure index, not a probability or a finding of wrongdoing.
- Make data coverage and freshness visible; never imply that every wallet or chain is monitored.

## Current stack

- **Frontend:** React, TypeScript, Vite, and Tailwind CSS.
- **API:** local-only Rust service built with Axum.
- **Persistent storage:** PostgreSQL, with an initial migration for investigations, tracked addresses, and risk signals.
- **Queue and coordination:** Redis Streams for the background sync worker; Redis is also checked by the API readiness endpoint.

The API supports investigation-scoped graph snapshots in addition to tracked addresses, history sync, risk signals, and exact-address screening. The graph combines persisted sender/recipient observations from EVM, TRON, Solana, and XRPL and can save/reopen a fixed view with its source observation IDs. Every hop is a separately observed event; the path does not prove that the same asset or funds moved end-to-end. Bitcoin inputs and outputs do not identify counterparties, so they are excluded from that graph. See the [graph snapshots API](docs/en/graph-snapshots-api.md) for request limits and routes. The overview graph and balances remain demo data. The API has no authentication and is intended for local development only.

- `GET` / `POST` `/api/v1/investigations`
- `GET` / `POST` `/api/v1/investigations/{investigation_id}/addresses`
- `POST` `/api/v1/investigations/{investigation_id}/addresses/{address_id}/sync`
- `GET` `/api/v1/investigations/{investigation_id}/addresses/{address_id}/transfers`
- `GET` / `POST` `/api/v1/investigations/{investigation_id}/addresses/{address_id}/risk-signals`
- `GET` / `PUT` / `DELETE` `/api/v1/investigations/{investigation_id}/addresses/{address_id}/risk-signals/{signal_id}`
- `GET` `/api/v1/screening/address?network=ethereum&address=0x...`
- `GET` / `POST` `/api/v1/investigations/{investigation_id}/graph-snapshots`

See [OFAC screening setup and coverage](docs/en/ofac-screening.md).

## Transaction history sync

Manual history sync remains available. The Compose worker periodically refreshes tracked addresses, targeting five minutes per address when provider capacity allows. Bitcoin uses Blockstream Esplora. EVM networks use the Blockscout mainnet catalog when that chain is present; Etherscan V2 supplements only chain IDs absent from the Blockscout snapshot. Etherscan coverage is restricted to an explicit mainnet allowlist, including BNB Smart Chain, Polygon, OP Mainnet, Avalanche C-Chain, Linea, Celo, Mantle, Gnosis, and other listed EVM networks. Network IDs remain `ethereum`, `base`, `arbitrum`, or `eip155:<chain_id>`. Etherscan sync needs a backend `ETHERSCAN_API_KEY`; BNB, OP, Avalanche, Gnosis, and other paid-tier networks also need `ETHERSCAN_ENABLE_PAID_CHAINS=true` and a key on an eligible plan. Free and paid chain availability follows the [current Etherscan chain list](https://docs.etherscan.io/supported-chains). Etherscan fetches native transactions, internal calls, and ERC-20 transfers, with bounded pagination and a per-key request throttle. The catalog marks provider availability and key/tier requirements; it does not guarantee provider health or complete history. Keys stay on the backend. See [continuous sync details](docs/en/continuous-sync.md).

Bitcoin sync keeps its existing bounded Esplora behavior. EVM sync fetches native transactions, successful internal native-value transfers, and ERC-20 transfer events in parallel. Raw integer amounts, token contract, optional decimals, event index, endpoints, time, block, and source host are stored. Each Blockscout stream is capped at 10 pages of 50 rows; each Etherscan stream at 10 pages of 100 rows. `may_have_more_history` indicates when the provider exposed another page at that cap. This is a bounded explorer view, not a complete index; provider coverage and omissions vary by chain. Multiple events in one transaction remain separate. The transfers endpoint returns up to 500 recent records. The background worker polls history; it does not stream every block or mempool event.

TRON mainnet is available through TronGrid. Set `TRONGRID_API_KEY` on the backend to send it as `TRON-PRO-API-KEY`; keep it out of browser configuration. The adapter stores successful native `TransferContract` transactions and TRC-20 `Transfer` events. This is bounded indexed history, not a complete chain index: provider retention and indexing can leave gaps, and TRC-10, TRC-721, internal contract TRX transfers, and other contract semantics are not decoded. Sync is capped at three pages of up to 200 rows per stream; 403/429 responses are retried on the next scheduled or manual sync. See [TRON sync details](docs/en/continuous-sync.md).

Solana is available as a separate network family through `SOLANA_RPC_URL` (defaults to the shared public Solana mainnet RPC). It stores finalized native System Program transfers and parsed SPL Token `transfer` / `transferChecked` instructions, including parsed inner instructions when token-account ownership and mint metadata are present. Sync advances in bounded pages of 15 signatures. SPL coverage is partial: `getSignaturesForAddress` returns transactions referencing the queried account key, so an incoming SPL transfer can be missed when the wallet appears only as token-balance owner metadata. The standard RPC flow also does not discover a wallet's full current and historical token-account set. Public RPC is rate-limited and may not retain old transactions; other programs, swaps, NFT/stake activity, and unsupported instructions are not covered. Use an indexed provider for broader owner-level token history and verify its coverage and terms. See [Solana sync details](docs/en/continuous-sync.md).

## Run the local API

Requirements: Docker Compose.

```sh
docker compose --env-file .env.example up --build -d
```

The example file uses local-development credentials. Compose publishes services on loopback; do not expose this unauthenticated API to a network. Check health at:

- `http://127.0.0.1:8080/health/live`
- `http://127.0.0.1:8080/health/ready` — verifies PostgreSQL and Redis.

Stop the services and preserve database data with:

```sh
docker compose --env-file .env.example down
```

## Run the frontend prototype

Requirements: Node.js 20.19+ or 22.12+ and npm.

```sh
npm ci
npm run dev
```

The app runs at the local URL printed by Vite. It currently uses bundled demo data and does not require an API key.

## Checks

Frontend checks:

```sh
npm run lint
npm run format:check
npm run test
npm run build
```

Rust API checks:

```sh
cargo fmt --manifest-path apps/api/Cargo.toml --all -- --check
cargo clippy --manifest-path apps/api/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path apps/api/Cargo.toml --all-features
```

## Documentation

- [Architecture](docs/en/architecture.md)
- [Risk model](docs/en/risk-model.md)
- [Risk signal API](docs/en/risk-signals-api.md)
- [OFAC address screening](docs/en/ofac-screening.md)
- [Continuous wallet sync](docs/en/continuous-sync.md)
- [Russian architecture](docs/ru/architecture.md) · [Russian risk model](docs/ru/risk-model.md)
- [Russian risk signal API](docs/ru/risk-signals-api.md)
- [Ukrainian architecture](docs/uk/architecture.md) · [Ukrainian risk model](docs/uk/risk-model.md)
- [Ukrainian risk signal API](docs/uk/risk-signals-api.md)
- [Russian OFAC screening guide](docs/ru/ofac-screening.md) · [Ukrainian OFAC screening guide](docs/uk/ofac-screening.md)

## Data sources and licensing

Bitcoin and selected EVM address sync read history from Blockstream Esplora and Blockscout on demand and through the Compose background worker. This provides blockchain transaction data, not identity attribution or comprehensive wallet-risk coverage. An optional local import supports exact address checks against the official OFAC SDN snapshot for Bitcoin and Ethereum; it refreshes only when an operator runs the importer. The API does not use OFAC's web search interface for automated queries ([OFAC FAQ](https://ofac.treasury.gov/faqs/287)). Commercial blockchain analytics require a separately licensed provider account.

Cardano mainnet history uses Blockfrost when `BLOCKFROST_PROJECT_ID` is configured on the backend. Sync is bounded to 82 logical provider operations (up to 164 HTTP attempts including one retry per 429/5xx) and uses Redis pacing. Cardano graph links are same-transaction participant evidence, not attributed transfers; see [Cardano coverage and limits](docs/en/continuous-sync.md).

## Contributing

Issues and pull requests are welcome. Before proposing a provider or scoring change, include its data provenance, license or terms, update behavior, and how an analyst can inspect the evidence. Keep observed facts and inferred relationships distinct.

This project is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the license text.

## Security and responsible use

Never submit private keys, seed phrases, or secrets. Tracefold is watch-only by design. Risk signals are leads for human review, not legal advice, proof of identity, or an automated basis for freezing or denying access to funds. Follow applicable law and provider terms when collecting or processing data.

- `GET` `/api/v1/networks`

TON mainnet history is available through TON Center API v3 for native messages and indexed Jetton transfers. Configure `TONCENTER_BASE_URL` and optionally `TONCENTER_API_KEY` on the backend. The bounded adapter requests at most 100 rows per stream page, uses separate head/backfill cursors, and stores Jetton raw units because decimals are not guaranteed. Provider indexing and retention can leave gaps; this is not a full chain index. See [TON sync details](docs/en/continuous-sync.md).

TON mainnet history is available through TON Center API v3 for native messages and indexed Jetton transfers. Configure `TONCENTER_BASE_URL` and optionally `TONCENTER_API_KEY` on the backend. The bounded adapter requests at most 100 rows per stream page, uses separate head/backfill cursors, and stores Jetton raw units because decimals are not guaranteed. Provider indexing and retention can leave gaps; this is not a full chain index. See [TON sync details](docs/en/continuous-sync.md).
