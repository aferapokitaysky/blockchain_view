# Architecture

## Current state

The repository currently contains a React, TypeScript, Vite, and Tailwind interface prototype. Wallets, graph edges, balances, transfer rows, network labels, and risk signals are static illustrative data in the frontend. There is no API connection, blockchain ingestion, identity attribution, or live provider integration yet.

## Target shape

```text
Chain RPC / indexed data / licensed intelligence providers
                       |
                 source adapters
                       |
          Rust API + background workers
                /              \
      PostgreSQL              Redis
   evidence and state   cache and coordination
                \              /
           React / TypeScript UI
```

- **React and TypeScript** render investigations and keep user interactions responsive.
- **Rust and Axum** are the planned API and processing layer. Parsing, normalization, and graph queries should live behind explicit service boundaries.
- **PostgreSQL** is the source of truth for investigations, wallets, transfers, source observations, and analyst annotations. Derived records should retain provenance and timestamps.
- **Redis** is intended for expiring cache entries, rate-limit coordination, and background-job coordination. It must not be the only store for evidence.
- **Source adapters** isolate chain-specific APIs and third-party datasets. Each adapter must expose coverage, last successful synchronization, licensing notes, and provider errors.

## Address and network identity

An address string alone is not a reliable cross-chain identity. Store a network identifier with every address and transaction. EVM addresses are syntactically compatible across many chains, so format recognition can return an EVM-family candidate but cannot reliably select Ethereum, Base, Arbitrum, or another EVM network. Let users confirm or select the chain unless an authoritative context resolves it.

Normalize addresses for comparisons according to the chain. Preserve the original displayed form and avoid merging entities solely because their address strings match.

## Evidence and graph model

A graph edge should point to a chain transaction and preserve direction, asset, amount, block or event time, ingestion time, and source. Labels and entity clusters should be separately stored claims with author, source, confidence, and observation time. Never rewrite an observed transfer to make it look like an analyst conclusion.

Queries should be bounded by network, time window, hop count, and result size. Paginate high-volume transaction lists and expose when a result is partial or stale.

## Reliability and safety

- Validate request payloads and enforce per-user investigation access in the API.
- Use timeouts and bounded retries for external providers; respect provider rate limits.
- Keep credentials server-side and never log them.
- Make ingestion idempotent using chain-specific transaction/event identifiers.
- Treat cached data as disposable and surface its age.
- Keep demo content visibly separate from provider-backed evidence.
