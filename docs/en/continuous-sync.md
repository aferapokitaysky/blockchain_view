# Continuous wallet history sync

The optional `sync-worker` Compose service periodically refreshes every tracked address through the existing Esplora and Blockscout adapters. The API remains available if the worker is stopped; manual sync endpoints continue to work.

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
