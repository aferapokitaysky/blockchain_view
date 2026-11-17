# Aptos history

The Aptos catalog entry uses the official hosted mainnet Indexer GraphQL endpoint (`APTOS_INDEXER_URL`, default `https://api.mainnet.aptoslabs.com/v1/graphql`). `APTOS_INDEXER_API_KEY` is optional and sent as a bearer token for providers that require it. The adapter requests at most 100 account transaction rows and 4 MiB per response.

The Indexer's `account_transactions` relation includes transactions that interact with an account. The adapter then reads fungible asset activity rows for those transaction versions. It stores only explicit Fungible Asset `Withdraw` and `Deposit` activities; raw integer units are preserved because asset decimals are not guaranteed. A counterparty is attached only when exactly one opposite activity has the same transaction version, asset type, and amount. Complex Move calls, NFTs, legacy coin events, and ambiguous multi-party activity are not attributed as direct transfers.

An Aptos address is normalized to a 32-byte lowercase `0x` hex value. Address format can identify Aptos but does not prove that an address is active. See [Aptos Indexer account transactions](https://aptos.dev/build/indexer/indexer-api/account-transactions) and [schema reference](https://aptos.dev/build/indexer/indexer-api/indexer-reference).
