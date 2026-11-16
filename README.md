# Tracefold

Open-source workspace for exploring how funds move between blockchain addresses.

> **Project status: early prototype.** The graph, wallet labels, balances, transaction table, and risk signals in the UI are illustrative demo data. The API can fetch Bitcoin address history on demand from Blockstream Esplora; it does not continuously monitor addresses. EVM sync, sanctions data, and commercial screening are not connected. Do not use demo values for operational or compliance decisions.

**Documentation:** [Русский](README.ru.md) · [Українська](README.uk.md) · [English](README.md)

## What it is

Tracefold is being designed as an OSINT investigation tool. An analyst should be able to start from an address, follow transfers across supported networks, preserve the evidence behind each relationship, and review risk signals without treating an algorithmic score as proof.

The interface prototype includes a wallet-flow graph, network filters, a recent-transfer table, and an explainable demo exposure score.

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
- **Cache and coordination:** Redis, checked by the API readiness endpoint.

The API supports creating and listing investigations, adding and listing tracked addresses, and manually syncing Bitcoin address history. The wallet form uses the API to save addresses and offers an on-demand sync action for Bitcoin. The graph, balances, risk panel, and transaction table remain demo data and do not render saved sync results. The API has no authentication and is intended for local development only. There is no continuous sync, EVM sync, sanctions feed, or commercial screening provider. The `risk_signals` table exists, but risk-signal API endpoints are not implemented.

- `GET` / `POST` `/api/v1/investigations`
- `GET` / `POST` `/api/v1/investigations/{investigation_id}/addresses`
- `POST` `/api/v1/investigations/{investigation_id}/addresses/{address_id}/sync`
- `GET` `/api/v1/investigations/{investigation_id}/addresses/{address_id}/transfers`

## Bitcoin transaction sync

Bitcoin sync is on demand only: call the sync endpoint for a tracked Bitcoin address, then read its stored results from the transfers endpoint. The provider is Blockstream Esplora. EVM transaction sync is unavailable.

Each sync fetches the initial address-history page, which can include current unconfirmed mempool transactions, then paginates confirmed history up to 20 pages total (up to 500 confirmed transactions). Results contain address-level input value spent and output value received per transaction, both in satoshis; these are totals, not a net balance change. The transfers endpoint returns up to 500 most recent stored records for that address. No background polling or continuous monitoring runs.

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
- [Russian architecture](docs/ru/architecture.md) · [Russian risk model](docs/ru/risk-model.md)
- [Ukrainian architecture](docs/uk/architecture.md) · [Ukrainian risk model](docs/uk/risk-model.md)

## Data sources and licensing

Bitcoin address sync reads transaction history from the public Blockstream Esplora API on demand. This provides blockchain transaction data, not identity attribution or comprehensive wallet-risk coverage. No sanctions or commercial screening dataset is connected. Future adapters must document provider terms, coverage, update cadence, attribution rules, and any redistribution limits. For sanctions screening, prefer machine-readable official datasets where their terms allow it; OFAC explicitly says its web search interface is intended for individual searches rather than continuous automated queries ([OFAC FAQ](https://ofac.treasury.gov/faqs/287)). Commercial blockchain analytics require a separately licensed provider account.

## Contributing

Issues and pull requests are welcome. Before proposing a provider or scoring change, include its data provenance, license or terms, update behavior, and how an analyst can inspect the evidence. Keep observed facts and inferred relationships distinct.

This project is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the license text.

## Security and responsible use

Never submit private keys, seed phrases, or secrets. Tracefold is watch-only by design. Risk signals are leads for human review, not legal advice, proof of identity, or an automated basis for freezing or denying access to funds. Follow applicable law and provider terms when collecting or processing data.
