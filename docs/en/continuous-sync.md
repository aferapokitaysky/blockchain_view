# Continuous wallet history sync

`docker compose up` starts the `sync-worker` service, which periodically refreshes every tracked address through the existing Esplora and Blockscout adapters. Stop it with `docker compose stop sync-worker` to disable background reads; the API and manual sync endpoints remain available.

## Network catalog and provider selection

`GET /api/v1/networks` returns Bitcoin plus the EVM mainnets currently published by Blockscout's `/api/json/config`. The API keeps the catalog in memory for six hours and falls back to the last successful catalog or the built-in Bitcoin, Ethereum, Base, Arbitrum, Solana, and TRON entries if refresh fails. Legacy IDs remain `bitcoin`, `ethereum`, `base`, and `arbitrum`; other EVM networks use `eip155:<chain_id>`. EVM address format does not identify a chain, so the analyst selects a network from this catalog.

For each chain, the API prefers a validated HTTPS Blockscout instance URL from `instance_url_from_config` or an explorer explicitly hosted by Blockscout. It does not send requests to arbitrary explorer URLs or follow redirects. If `BLOCKSCOUT_API_KEY` is set, the configured multichain API base is used instead; a custom `BLOCKSCOUT_BASE_URL` also acts as an explicit provider override. The `history_sync_available` value means a provider route is configured, not that the endpoint was health-checked. Individual instances may rate-limit, block automated requests, return HTML challenges, or omit history; those responses become a generic provider-unavailable error and are not stored as observations.

## TRON history and coverage

The built-in `tron` network uses TronGrid mainnet at `https://api.trongrid.io`. Set `TRONGRID_BASE_URL` to another trusted TronGrid-compatible root URL if needed, and optionally set `TRONGRID_API_KEY`. The key is sent only in the backend `TRON-PRO-API-KEY` header; it is never part of a request URL or frontend configuration. `TRON-PRO-API-KEY` must not be exposed in browser code.

Two independent fingerprint cursors are maintained for native account transactions and TRC-20 event history. Each sync is bounded to three pages per stream (TronGrid page size 200); incomplete catch-up/backfill continues on a later sync. The account endpoint includes contract calls and other transaction types, so the adapter stores native TRX only for successful `TransferContract` entries. TRC-20 history stores rows whose event type is exactly `Transfer`; approvals and other events are ignored. Source addresses are checksum-validated and normalized before matching. When the provider does not return a documented event/log index, a deterministic event identity is synthesized so multiple events in one transaction are retained.

This is indexed provider history, not a complete chain index. TronGrid can rate-limit, omit or delay indexed rows, and older history availability depends on provider retention. TRX internal contract transfers, TRC-10, TRC-721, swaps, and other contract semantics are not decoded. HTTP 403/429 responses fail the current sync without an immediate retry; the worker retries on a later scheduled run, or a user can request another manual sync. The built-in catalog reports only `native-trx` and `trc20` coverage and marks the route available when the adapter is configured, not health-checked.

## Solana history and coverage

The built-in Solana entry uses `SOLANA_RPC_URL`, defaulting to `https://api.mainnet.solana.com`. Set it to an HTTPS RPC endpoint from a provider you operate or trust. HTTP is accepted only for loopback development endpoints. The catalog marks the route as configured; it does not probe provider health or guarantee archival retention.

Sync uses Solana JSON-RPC `getSignaturesForAddress` at `finalized` commitment, then retrieves each transaction with `getTransaction` using `jsonParsed` and `maxSupportedTransactionVersion: 0`. Each invocation scans at most 15 signatures and persists separate forward/backfill cursors. It stores parsed System Program `transfer` and `transferWithSeed` native SOL events, and SPL Token / Token-2022 `transfer` and `transferChecked` events from top-level and inner instructions, in instruction order. SPL ownership is attributed from matching `preTokenBalances` / `postTokenBalances`; ambiguous records without owner or mint metadata are skipped. Raw integer amounts are stored, and token decimals are retained when returned by the RPC.

This is intentionally not a complete Solana index. The official [`getSignaturesForAddress`](https://solana.com/docs/rpc/http/getsignaturesforaddress) method searches transactions whose `accountKeys` reference the queried public key. An incoming SPL transaction can mention the wallet only as `owner` in token-balance metadata while its token account is the referenced key; that transaction will not be discovered by querying the owner address. The standard RPC flow also does not enumerate a wallet's complete current or historical token-account set, and closed token accounts cannot be reconstructed by this scanner. Other programs (including swaps, NFTs, and staking) and nonstandard instructions are not decoded. Public RPC is shared infrastructure and can rate-limit or block requests; an HTTP error fails the current sync and is retried on a later manual or scheduled sync, while providers may also prune older ledger data. For broader owner-level or historical token coverage, configure an indexed provider after verifying its coverage and terms. The [`getTransaction`](https://solana.com/docs/rpc/http/gettransaction) request uses the documented transaction version `0`.

## XRP Ledger history and coverage

The built-in `xrpl` network uses `XRPL_RPC_URL`, defaulting to Ripple's public full-history endpoint `https://s2.ripple.com:51234/`. Configure a trusted HTTPS JSON-RPC endpoint for regular use. Sync uses `account_tx` marker pagination with independent cursors: a fresh head scan finds recent transactions, a pending head marker continues catch-up until the last saved transaction is found, and a separate bounded backfill walks older pages. Each run scans at most two fresh head pages, one pending head page, and one backfill page, with up to 50 transactions per page. The head checkpoint advances only after the previous checkpoint is reached. Only checksum-validated classic `r...` account addresses are accepted; X-addresses are not.

The adapter stores only successful, validated, direct XRP Payment events for the tracked transaction `Account` or `Destination`. The amount comes from metadata `delivered_amount`, so partial payments do not use the requested `Amount`; fee-only changes, XRP reserve changes, failed/unvalidated transactions, path payments, issued-currency payments, and non-Payment transaction types are skipped. This deliberately limited v1 does not claim full wallet activity coverage or decode IOU trust-line balance changes.

`account_tx` only returns history retained by the selected server. The public endpoint may rate-limit or be unavailable and is not intended for sustained production traffic; configure an operated or contracted archival endpoint for regular monitoring. The catalog availability flag indicates a configured route, not complete ledger retention. See the [official account_tx reference](https://xrpl.org/docs/references/http-websocket-apis/public-api-methods/account-methods/account_tx) and [ledger history guide](https://xrpl.org/docs/concepts/networks-and-servers/ledger-history).

```bash
docker compose up --build
```

Run it outside Compose with the same `DATABASE_URL`, `REDIS_URL`, `BITCOIN_ESPLORA_URL`, `XRPL_RPC_URL`, and Blockscout variables:

```bash
cargo run --manifest-path apps/api/Cargo.toml --bin sync_worker
```

The scheduler scans addresses in keyset batches of 1,000 every 30 seconds. Redis Streams holds at most 10,000 queued or pending jobs. Each address is deduplicated, jobs are acknowledged after idempotent PostgreSQL upserts, and abandoned jobs are reclaimed after 10 minutes. Provider and database failures use exponential retry delays from 30 seconds up to one hour. A sync attempt is capped at eight minutes. SIGINT and SIGTERM stop scheduling and let an active job finish before exit.

The target interval is five minutes per address when the provider and worker have enough capacity. A large backlog or provider rate limits can make the actual interval longer. History depth remains bounded by the existing adapters: Bitcoin sync is capped at 20 pages; each EVM stream at 10 pages. The worker does not provide mempool-wide monitoring, identity attribution, key custody, or transaction signing. There is no per-address pause control yet; adding an address enables periodic reads from configured public data providers.

The queue is at-least-once. Duplicate jobs are safe because observation writes use database uniqueness keys and upserts. Redis persistence is enabled in Compose; keep the Redis volume when restarting deployments.

## TON history and coverage

The built-in `ton` network uses TON Center API v3 at `https://toncenter.com/api/v3`. Configure `TONCENTER_BASE_URL` and optionally `TONCENTER_API_KEY` in the backend environment. The key is sent only in `X-API-Key`; the documented limit without a key is one request per second. Mainnet friendly addresses are checksum-validated and stored as canonical `workchain:hex`; raw `0:` and `-1:` addresses are also accepted.

Native messages and indexed Jetton transfers use `/transactions` and `/jetton/transfers` separately. Each request is capped at 100 rows; head refresh and historical backfill maintain independent logical-time cursors and process at most one page in each direction per stream per sync. Jetton amounts remain raw integer units because the transfer endpoint does not guarantee token decimals. This is indexed provider history, not a complete chain index: retention, indexing delay, rate limits and unsupported message semantics can leave gaps. Internal contract behavior, swaps and arbitrary payloads are not interpreted. See the official [TON Center v3 overview](https://docs.ton.org/ecosystem/api/toncenter/v3/overview), [transactions endpoint](https://docs.ton.org/api/v3/blockchain-data/get-transactions), and [Jetton transfers endpoint](https://docs.ton.org/ecosystem/api/toncenter/v3/jettons/get-jetton-transfers).
