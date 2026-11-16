# Tracefold

Open-source workspace for exploring how funds move between blockchain addresses.

> **Project status: early prototype.** The current graph, wallet labels, transactions, balances, and risk signals are illustrative demo data. No blockchain indexer, sanctions feed, or commercial screening provider is connected. Do not use the displayed numbers for operational or compliance decisions.

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

## Planned stack

- **Frontend:** React, TypeScript, Vite, Tailwind CSS.
- **API and processing:** Rust with Axum.
- **Persistent records:** PostgreSQL.
- **Cache and short-lived coordination:** Redis.

The backend, database schema, provider adapters, and production risk model are not complete yet.

## Run the frontend prototype

Requirements: Node.js 20.19+ or 22.12+ and npm.

```sh
npm ci
npm run dev
```

The app runs at the local URL printed by Vite. It currently uses bundled demo data and does not require an API key.

## Checks

```sh
npm run lint
npm run format:check
npm run test
npm run build
```

Rust checks will be documented when the API crate lands.

## Documentation

- [Architecture](docs/en/architecture.md)
- [Risk model](docs/en/risk-model.md)
- [Russian architecture](docs/ru/architecture.md) · [Russian risk model](docs/ru/risk-model.md)
- [Ukrainian architecture](docs/uk/architecture.md) · [Ukrainian risk model](docs/uk/risk-model.md)

## Data sources and licensing

No third-party attribution or sanctions dataset is bundled in this prototype. Future adapters must document provider terms, coverage, update cadence, attribution rules, and any redistribution limits. For sanctions screening, prefer machine-readable official datasets where their terms allow it; OFAC explicitly says its web search interface is intended for individual searches rather than continuous automated queries ([OFAC FAQ](https://ofac.treasury.gov/faqs/287)). Commercial blockchain analytics require a separately licensed provider account.

## Contributing

Issues and pull requests are welcome. Before proposing a provider or scoring change, include its data provenance, license or terms, update behavior, and how an analyst can inspect the evidence. Keep observed facts and inferred relationships distinct.

The repository has not selected a license yet. A license file must be added before others can safely reuse or redistribute the code.

## Security and responsible use

Never submit private keys, seed phrases, or secrets. Tracefold is watch-only by design. Risk signals are leads for human review, not legal advice, proof of identity, or an automated basis for freezing or denying access to funds. Follow applicable law and provider terms when collecting or processing data.
