# Risk model

## Current implementation

The UI loads analyst-entered signals for the selected saved address, derives behavior signals from confirmed saved observations, and can add exact OFAC SDN matches after an explicit screening request. The deterministic `0.3.0` model weights signal severity, category, and hop distance, caps the sum at 100, and displays each contribution. The displayed value is **not** a calibrated probability, a finding that funds are illicit, or a compliance decision.

The API stores analyst-entered signals with source names, references, observation timestamps, and structured details. They are not automatically verified. Until evidence is explicitly entered or an OFAC exact match is returned, the score does not include it.

Freshness, source reliability, and independent corroboration remain visible context and do not affect the score. The model does not infer confidence from missing metadata. An OFAC `no_exact_match`, unavailable snapshot, or unsupported network adds no points and does not reduce other evidence.

## Current calculation (`0.3.0`)

Severity weights are critical 80, high 48, medium 24, low 8, and informational 0. Direct evidence uses a distance factor of 1; hop `h > 0` uses `1 / (h + 1)` (one hop is one half, two hops one third). Category factors are sanctions 1, behavior 0.7, exposure 0.55, and other 0. Each row shows its base severity, distance, category, and point contribution. Contributions are summed, capped at 100, and rounded for display. An exact OFAC address match is represented as direct critical sanctions evidence, only after the user runs the local snapshot check. Analyst-entered `details.hops` or `details.hop_count` supplies path distance; malformed values fall back to direct evidence because the API record itself is scoped to the wallet. Do not enter indirect evidence without a source-backed path.

The displayed bands are low below 20, guarded from 20, elevated from 45, and high from 75. They are review priorities, not calibrated probabilities.

The panel is address-scoped, reports the model version, and describes an empty signal set as “no evidence recorded,” not a clean result. Source timestamps and references are displayed but not used in the calculation. Stale evidence is not down-weighted yet.

Confirmed saved observations also produce derived fan-in and fan-out signals when at least four distinct counterparties appear in the same rolling 24-hour window. Incoming and outgoing directions are evaluated separately. Unconfirmed events, missing counterparties, and invalid timestamps are ignored. Each pattern contributes once per selected address and direction; duplicate events with the same counterparty do not increase the count. The signal lists its network, provider, and concrete UTC start/end timestamps, and explicitly does not imply common ownership or intent. These are deterministic triage heuristics, not calibrated indicators; the threshold is provisional and should be evaluated against labeled cases before operational use.

A rapid-turnover signal requires at least two non-overlapping inbound→outbound pairs for the same network and asset inside a rolling 24-hour window. Each leg must be confirmed and have an on-chain timestamp; the outbound event must follow the inbound event within 10 minutes, use a different transaction, and fall between 70% and 130% of the inbound raw amount. Pairing uses integer arithmetic and each event can be used once. The signal is low for two or three pairs and medium for four or more; it shows up to three concrete transaction pairs, their provider, the formatted amount, and the original raw integer. Similar timing and amounts do not prove that the same funds continued or establish intent.

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

An exact-address check against a locally imported OFAC SDN snapshot is implemented for Bitcoin and Ethereum; see [screening setup and coverage](ofac-screening.md). No commercial provider or indirect-exposure model is connected. OFAC publishes machine-readable sanctions list files for automated use; its interactive search tool is intended for individual lookups, not automated continuous queries ([OFAC FAQ](https://ofac.treasury.gov/faqs/287)). Before adding another provider, review its API terms, redistribution rights, supported networks, refresh cadence, and attribution requirements.

## Providers and public datasets to evaluate

These are research candidates only; **no integration is implemented or selected**. Coverage, terms, reliability, and cost need independent evaluation.

- [OFAC Sanctions List Service](https://ofac.treasury.gov/sanctions-list-service) publishes machine-readable SDN and consolidated non-SDN datasets. OFAC says its interactive [Sanctions List Search](https://ofac.treasury.gov/faqs/287) is for individual searches, not continuous automated use. Public sanctions data does not provide comprehensive illicit-address attribution.
- Commercial screening products to evaluate: [Chainalysis KYT](https://www.chainalysis.com/product/kyt/), [TRM Wallet Screening](https://www.trmlabs.com/blockchain-intelligence-platform/wallet-screening), and [Elliptic Lens](https://www.elliptic.co/products/lens/). Access requires vendor evaluation and may require a commercial contract.
- Chain data ingestion is a separate dependency: network RPC or an indexed-data provider must be selected for each chain.

## User interface language

Prefer “risk signals”, “exposure”, “source”, “observed”, and “needs review”. Avoid labeling an address “criminal”, “dirty”, or “sanctioned” solely from an indirect or unverified signal. Show network, source, evidence age, path distance, and uncertainty beside any score.
