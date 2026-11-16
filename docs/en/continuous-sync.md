# Continuous wallet history sync

`docker compose up` starts the `sync-worker` service, which periodically refreshes every tracked address through the existing Esplora and Blockscout adapters. Stop it with `docker compose stop sync-worker` to disable background reads; the API and manual sync endpoints remain available.

## Network catalog and provider selection

`GET /api/v1/networks` returns Bitcoin plus the EVM mainnets currently published by Blockscout's `/api/json/config`. The API keeps the catalog in memory for six hours and falls back to the last successful catalog or the built-in Bitcoin, Ethereum, Base, and Arbitrum entries if refresh fails. Legacy IDs remain `bitcoin`, `ethereum`, `base`, and `arbitrum`; other EVM networks use `eip155:<chain_id>`. EVM address format does not identify a chain, so the analyst selects a network from this catalog.

For each chain, the API prefers a validated HTTPS Blockscout instance URL from `instance_url_from_config` or an explorer explicitly hosted by Blockscout. It does not send requests to arbitrary explorer URLs or follow redirects. If `BLOCKSCOUT_API_KEY` is set, the configured multichain API base is used instead; a custom `BLOCKSCOUT_BASE_URL` also acts as an explicit provider override. The `history_sync_available` value means a provider route is configured, not that the endpoint was health-checked. Individual instances may rate-limit, block automated requests, return HTML challenges, or omit history; those responses become a generic provider-unavailable error and are not stored as observations.

```bash
docker compose up --build
```

Run it outside Compose with the same `DATABASE_URL`, `REDIS_URL`, `BITCOIN_ESPLORA_URL`, and Blockscout variables:

```bash
cargo run --manifest-path apps/api/Cargo.toml --bin sync_worker
```

The scheduler scans addresses in keyset batches of 1,000 every 30 seconds. Redis Streams holds at most 10,000 queued or pending jobs. Each address is deduplicated, jobs are acknowledged after idempotent PostgreSQL upserts, and abandoned jobs are reclaimed after 10 minutes. Provider and database failures use exponential retry delays from 30 seconds up to one hour. A sync attempt is capped at eight minutes. SIGINT and SIGTERM stop scheduling and let an active job finish before exit.

The target interval is five minutes per address when the provider and worker have enough capacity. A large backlog or provider rate limits can make the actual interval longer. History depth remains bounded by the existing adapters: Bitcoin sync is capped at 20 pages; each EVM stream at 10 pages. The worker does not provide mempool-wide monitoring, identity attribution, key custody, or transaction signing. There is no per-address pause control yet; adding an address enables periodic reads from configured public data providers.

The queue is at-least-once. Duplicate jobs are safe because observation writes use database uniqueness keys and upserts. Redis persistence is enabled in Compose; keep the Redis volume when restarting deployments.
