# Contributing

Thank you for helping improve Tracefold. Contributions should keep observed on-chain facts separate from inferred relationships and risk signals.

Translations: [Русский](docs/ru/contributing.md) · [Українська](docs/uk/contributing.md)

## Before opening a change

- Search existing issues and code for the behavior you want to change.
- Open an issue for substantial provider, schema, or scoring changes so the data source and expected coverage can be discussed first.
- Never include private keys, seed phrases, provider credentials, real user data, or unredacted logs in issues, tests, or fixtures.

## Development setup

Use Node.js 22, Rust stable, Docker Compose, PostgreSQL, and Redis. Copy `.env.example` to `.env` only for local development and put real provider credentials in the ignored `.env` file.

```sh
npm ci
docker compose up --build
```

The frontend runs with `npm run dev`. The compose stack starts the API, PostgreSQL, Redis, and sync worker. Read [architecture](docs/en/architecture.md) and [continuous sync](docs/en/continuous-sync.md) before changing data flow or provider behavior.

## Change and verification

Keep each pull request focused. Add migrations for schema changes and tests for business rules, provider parsing, cursor behavior, and important error cases. Provider changes should document their source, licensing or terms, rate limits, pagination, and known coverage gaps.

Run the checks relevant to your change before opening a PR:

```sh
npm run lint
npm run format:check
npm run test
npm run build
cargo fmt --manifest-path apps/api/Cargo.toml --all -- --check
cargo clippy --manifest-path apps/api/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path apps/api/Cargo.toml --all-features
```

PostgreSQL and Redis must be available for the full Rust integration suite. GitHub Actions runs the frontend checks and the Rust API checks on every pull request.

## Pull request checklist

- Describe the user-visible behavior and any provider coverage limits.
- Link the issue or explain why the change is needed.
- Include tests and update EN/RU/UK docs when behavior, configuration, API, or coverage changes.
- Confirm that no credentials, private keys, or personal data were added.
- Keep comments and UI copy factual; risk scores are review-priority signals, not findings of wrongdoing.

For vulnerability reports, follow [SECURITY.md](SECURITY.md). The project is licensed under [Apache-2.0](LICENSE).
