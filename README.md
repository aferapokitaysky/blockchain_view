# Tracefold

Open-source workspace for exploring how funds move between blockchain addresses.

> **Project status: early prototype.** The overview graph, wallet labels, and balances remain illustrative demo data. The recent-transactions table reads persisted observations for saved wallets and stays empty until history is synced through a configured provider. The risk panel loads analyst-entered signals for the selected saved address and may include an exact OFAC SDN match after an explicit local snapshot check; its versioned score is provisional, not a legal conclusion or complete compliance check. Manual sync is available in the saved-wallet panel. `docker compose up` also starts `sync-worker`, which periodically refreshes tracked Bitcoin and supported EVM addresses, targeting five minutes per address when provider capacity allows; this is bounded history polling, not real-time or full mempool monitoring. The separate transfer-links view explores directed paths up to three hops using only saved EVM sender/recipient events; each hop is independently observed and a path does not establish that the same funds continued across hops. Bitcoin counterparties are not inferred. EVM history support is limited to Ethereum, Base, and Arbitrum One. Exact-address screening against a locally imported OFAC SDN snapshot is available for Bitcoin and Ethereum. Do not use demo values for operational or compliance decisions.

**Documentation:** [Русский](README.ru.md) · [Українська](README.uk.md) · [English](README.md)

## What it is

Tracefold is being designed as an OSINT investigation tool. An analyst should be able to start from an address, follow transfers across supported networks, preserve the evidence behind each relationship, and review risk signals without treating an algorithmic score as proof.

The interface prototype includes a demo overview graph and an evidence-based EVM transfer-links view, alongside network filters, a persisted-observation table, and a versioned, explainable assessment for the selected saved wallet.

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

The API supports creating and listing investigations, adding and listing tracked addresses, manually syncing Bitcoin and catalogued EVM mainnet history, investigation-scoped CRUD for analyst-entered risk signals, and exact-address screening against an imported OFAC SDN snapshot. `GET /api/v1/networks` lists available networks and whether a provider route is configured. The recent-transactions table shows persisted observations and their source. The risk panel loads signals for the selected saved wallet and shows the versioned provisional score with per-signal contributions; an exact OFAC match contributes direct sanctions evidence only after an explicit check, while a non-match never lowers the score. The Compose worker refreshes tracked addresses through the same bounded provider adapters; see [continuous sync](docs/en/continuous-sync.md). The transfer-links view explores directed EVM paths up to three hops from persisted sender/recipient events and labels untracked counterparties. Every hop is a separately observed event; the path does not prove that the same asset or funds moved end-to-end. Bitcoin inputs and outputs do not identify counterparties, so they are excluded from that graph. The overview graph and balances remain demo data. The API has no authentication and is intended for local development only. The OFAC snapshot is refreshed manually; no commercial screening provider is connected.

- `GET` / `POST` `/api/v1/investigations`
- `GET` / `POST` `/api/v1/investigations/{investigation_id}/addresses`
- `POST` `/api/v1/investigations/{investigation_id}/addresses/{address_id}/sync`
- `GET` `/api/v1/investigations/{investigation_id}/addresses/{address_id}/transfers`
- `GET` / `POST` `/api/v1/investigations/{investigation_id}/addresses/{address_id}/risk-signals`
- `GET` / `PUT` / `DELETE` `/api/v1/investigations/{investigation_id}/addresses/{address_id}/risk-signals/{signal_id}`
- `GET` `/api/v1/screening/address?network=ethereum&address=0x...`

See [OFAC screening setup and coverage](docs/en/ofac-screening.md).

## Transaction history sync

Manual history sync remains available. The Compose worker periodically refreshes tracked addresses, targeting five minutes per address when provider capacity allows. Bitcoin uses Blockstream Esplora. EVM networks come from Blockscout's mainnet catalog at `GET /api/v1/networks`; existing IDs remain `ethereum`, `base`, and `arbitrum`, while other chains use `eip155:<chain_id>`. The API uses a validated Blockscout instance URL from the catalog when available. A listed sync endpoint is configured, not a live health guarantee: public explorers can rate-limit, challenge, or omit data. Set `BLOCKSCOUT_API_KEY` to use the Blockscout multichain API; a custom `BLOCKSCOUT_BASE_URL` overrides per-instance URLs. The key is sent only from the backend. See [continuous sync details](docs/en/continuous-sync.md).

Bitcoin sync keeps its existing bounded Esplora behavior. EVM sync fetches native transactions, successful internal native-value transfers, and ERC-20 transfer events in parallel. Raw integer amounts, token contract, optional decimals, event index, endpoints, time, block, and source host are stored. Each EVM stream is capped at 10 pages of 50 rows; `may_have_more_history` indicates when the provider exposed another page at that cap. This is a bounded explorer view, not a complete index; provider coverage and omissions vary by chain. Multiple events in one transaction remain separate. The transfers endpoint returns up to 500 recent records. The background worker polls history; it does not stream every block or mempool event.

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

## Contributing

Issues and pull requests are welcome. Before proposing a provider or scoring change, include its data provenance, license or terms, update behavior, and how an analyst can inspect the evidence. Keep observed facts and inferred relationships distinct.

This project is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the license text.

## Security and responsible use

Never submit private keys, seed phrases, or secrets. Tracefold is watch-only by design. Risk signals are leads for human review, not legal advice, proof of identity, or an automated basis for freezing or denying access to funds. Follow applicable law and provider terms when collecting or processing data.

- `GET` `/api/v1/networks`
