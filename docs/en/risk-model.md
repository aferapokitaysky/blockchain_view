# Risk model

## Current demonstration

The UI currently computes a small illustrative exposure index from bundled sample signals. The implementation weights signal severity, category, and hop distance, then caps the sum at 100. The displayed value is **not** a calibrated probability, a finding that funds are illicit, or a compliance decision. The sample evidence is synthetic and is not returned by a provider.

Freshness, source reliability, and independent corroboration are displayed as source/time fields in the demo model but do not yet affect the score. The production design must not silently treat those fields as equivalent.

## Intended evidence model

A score should be explainable as a set of versioned observations, for example:

- a direct address or service attribution from a named source;
- an indirect path to an attributed service, with chain, time range, and hop count;
- a transaction pattern such as rapid pass-through or unusually broad fan-out;
- contradictory or stale evidence that lowers confidence or triggers review.

Each observation should retain its provider, dataset/version, source reference where redistribution permits, observed and ingested timestamps, chain, address or transaction, path distance, confidence, and expiration/retraction state.

## Planned calculation

A future model may combine severity, path distance, source quality, corroboration, and evidence age. Keep the calculation deterministic and versioned so the same evidence can be reproduced. Store component contributions next to the total and explain each contribution in the interface. Avoid counting duplicates from the same underlying source as independent corroboration.

Thresholds must be configurable and evaluated against a documented validation set. A score range is a prioritization band, not a probability, unless calibration has actually been demonstrated. Always provide a way to inspect evidence and record an analyst disposition.

## Sources

No list or commercial provider is connected. OFAC publishes machine-readable sanctions list files for automated use; its interactive search tool is intended for individual lookups, not automated continuous queries ([OFAC FAQ](https://ofac.treasury.gov/faqs/287)). Before adding a provider, review its API terms, redistribution rights, supported networks, refresh cadence, and attribution requirements.

## Providers and public datasets to evaluate

These are research candidates only; **no integration is implemented or selected**. Coverage, terms, reliability, and cost need independent evaluation.

- [OFAC Sanctions List Service](https://ofac.treasury.gov/sanctions-list-service) publishes machine-readable SDN and consolidated non-SDN datasets. OFAC says its interactive [Sanctions List Search](https://ofac.treasury.gov/faqs/287) is for individual searches, not continuous automated use. Public sanctions data does not provide comprehensive illicit-address attribution.
- Commercial screening products to evaluate: [Chainalysis KYT](https://www.chainalysis.com/product/kyt/), [TRM Wallet Screening](https://www.trmlabs.com/blockchain-intelligence-platform/wallet-screening), and [Elliptic Lens](https://www.elliptic.co/products/lens/). Access requires vendor evaluation and may require a commercial contract.
- Chain data ingestion is a separate dependency: network RPC or an indexed-data provider must be selected for each chain.

## User interface language

Prefer “risk signals”, “exposure”, “source”, “observed”, and “needs review”. Avoid labeling an address “criminal”, “dirty”, or “sanctioned” solely from an indirect or unverified signal. Show network, source, evidence age, path distance, and uncertainty beside any score.
