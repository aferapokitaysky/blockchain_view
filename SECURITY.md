# Security policy

Tracefold is watch-only: it does not request or store private keys, seed phrases, or signing credentials. Do not include those secrets or real wallet-owner data in reports.

Translations: [Русский](docs/ru/security.md) · [Українська](docs/uk/security.md)

## Reporting a vulnerability

Please do not publish exploitable details in a public issue or pull request. Use [GitHub's private vulnerability reporting form](https://github.com/aferapokitaysky/blockchain_view/security/advisories/new) to contact maintainers privately.

Include the affected version or commit, impact, reproduction conditions, and a suggested mitigation when safe. Reports are reviewed by maintainers; this project does not promise a response time or a security bounty.

## Scope and data handling

Security reports may cover the API, frontend, synchronization worker, database migrations, provider configuration, and CI workflows. Provider outages, unsupported chain history, heuristic false positives, and third-party service behavior are not vulnerabilities by themselves; report them as reliability or coverage issues unless they expose data or enable an attack.

Never send funds, keys, or transactions to demonstrate an issue. Use local test data and redact wallet addresses when they are not necessary to reproduce the problem.
