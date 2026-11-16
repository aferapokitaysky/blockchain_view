# OFAC SDN address screening

Tracefold can screen an address for an exact match in a locally imported OFAC Specially Designated Nationals (SDN) snapshot. It does not call OFAC for each request and does not send wallet addresses to OFAC.

## Import the current snapshot

The API must be able to connect to PostgreSQL. From the repository root, run:

```bash
DATABASE_URL=postgres://tracefold:tracefold_dev_only@localhost:5432/tracefold \
  cargo run --manifest-path apps/api/Cargo.toml --bin import_ofac_sdn
```

The command downloads OFAC's `SDN.XML`, checks the XML size and structure, then atomically replaces the stored address snapshot. A failed download or parse leaves the previous snapshot intact. To import a file you downloaded separately, pass its path:

```bash
DATABASE_URL=postgres://tracefold:tracefold_dev_only@localhost:5432/tracefold \
  cargo run --manifest-path apps/api/Cargo.toml --bin import_ofac_sdn -- /path/to/SDN.XML
```

Run the import again when you want to refresh the data. There is no scheduled update. The command uses the official [OFAC Sanctions List Service](https://ofac.treasury.gov/sanctions-list-service) endpoint. OFAC's service may return HTTP 403 to automated downloads without a `User-Agent`; the importer sends one as described in its [technical notice](https://ofac.treasury.gov/sdn-list-data-formats-data-schemas/ofac-technical-actions-in-reverse-chronological-order/20240516_44).

## Query an address

```http
GET /api/v1/screening/address?network=ethereum&address=0x...
```

The endpoint returns the source name, list publication value, local import time, and exact matches. It supports Bitcoin (`XBT`) and Ethereum (`ETH`). Base and Arbitrum return `network_not_covered`, because OFAC's `ETH` currency identifier does not establish that the entry applies to those networks.

Possible `status` values:

- `exact_match`: this address string exactly matches one or more entries for the selected currency in the imported snapshot.
- `no_exact_match`: the address was not present in that snapshot for that currency.
- `source_not_imported`: no local snapshot is available. This is not a negative result.
- `network_not_covered`: the endpoint does not map this network to an OFAC currency identifier.

OFAC describes digital currency address lookup as exact-match search. Tracefold does not use fuzzy matching, infer identity, follow transaction hops, or add these results to the demo score. A non-match is limited to this source, currency, and snapshot. See OFAC's [virtual currency address FAQ](https://ofac.treasury.gov/faqs/594) and [address format FAQ](https://ofac.treasury.gov/faqs/563).

The API has no authentication and is intended for local development. Do not expose it to an untrusted network.
