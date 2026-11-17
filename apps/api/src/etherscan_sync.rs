use std::{collections::HashMap, env, sync::Arc, time::Duration};

use chrono::{DateTime, NaiveDate, Utc};
use reqwest::{Client, Url};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use tokio::{sync::Mutex, time::Instant};
use tracing::warn;

use crate::{error::ApiError, evm_sync::EvmTransfer};

const API_URL: &str = "https://api.etherscan.io/v2/api";
const MAX_PAGES: u32 = 10;
const PAGE_SIZE: u32 = 100;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(400);
const REDIS_REQUEST_GATE_KEY: &str = "tracefold:etherscan:v2:request-gate";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EtherscanNetwork {
    pub chain_id: u64,
    pub name: &'static str,
    pub native_asset: &'static str,
    pub explorer_url: &'static str,
    pub tier: EtherscanTier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EtherscanTier {
    Free,
    Paid,
    FreeUntil { year: i32, month: u32, day: u32 },
}

impl EtherscanTier {
    pub(crate) fn requires_paid_tier_on(self, as_of: NaiveDate) -> bool {
        match self {
            Self::Free => false,
            Self::Paid => true,
            Self::FreeUntil { year, month, day } => {
                as_of
                    > NaiveDate::from_ymd_opt(year, month, day)
                        .expect("provider tier cutoff is a valid date")
            }
        }
    }
}

// Явный список не позволяет превратить переданный пользователем chain ID в адрес запроса к провайдеру.
pub(crate) const SUPPORTED_NETWORKS: &[EtherscanNetwork] = &[
    EtherscanNetwork {
        chain_id: 56,
        name: "BNB Smart Chain",
        native_asset: "BNB",
        explorer_url: "https://bscscan.com/",
        tier: EtherscanTier::Paid,
    },
    EtherscanNetwork {
        chain_id: 137,
        name: "Polygon PoS",
        native_asset: "POL",
        explorer_url: "https://polygonscan.com/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 10,
        name: "OP Mainnet",
        native_asset: "ETH",
        explorer_url: "https://optimistic.etherscan.io/",
        tier: EtherscanTier::Paid,
    },
    EtherscanNetwork {
        chain_id: 43114,
        name: "Avalanche C-Chain",
        native_asset: "AVAX",
        explorer_url: "https://snowscan.xyz/",
        tier: EtherscanTier::Paid,
    },
    EtherscanNetwork {
        chain_id: 59144,
        name: "Linea",
        native_asset: "ETH",
        explorer_url: "https://lineascan.build/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 42220,
        name: "Celo",
        native_asset: "CELO",
        explorer_url: "https://celoscan.io/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 5000,
        name: "Mantle",
        native_asset: "MNT",
        explorer_url: "https://mantlescan.xyz/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 100,
        name: "Gnosis",
        native_asset: "xDAI",
        explorer_url: "https://gnosisscan.io/",
        tier: EtherscanTier::Paid,
    },
    EtherscanNetwork {
        chain_id: 199,
        name: "BitTorrent Chain",
        native_asset: "BTT",
        explorer_url: "https://bttcscan.com/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 252,
        name: "Fraxtal",
        native_asset: "frxETH",
        explorer_url: "https://fraxscan.com/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 4352,
        name: "MemeCore",
        native_asset: "M",
        explorer_url: "https://memecorescan.io/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 167000,
        name: "Taiko",
        native_asset: "ETH",
        explorer_url: "https://taikoscan.io/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 50,
        name: "XDC Network",
        native_asset: "XDC",
        explorer_url: "https://xdcscan.com/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 480,
        name: "World Chain",
        native_asset: "ETH",
        explorer_url: "https://worldscan.org/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 146,
        name: "Sonic",
        native_asset: "S",
        explorer_url: "https://sonicscan.org/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 130,
        name: "Unichain",
        native_asset: "ETH",
        explorer_url: "https://uniscan.xyz/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 80094,
        name: "Berachain",
        native_asset: "BERA",
        explorer_url: "https://berascan.com/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 143,
        name: "Monad",
        native_asset: "MON",
        explorer_url: "https://monadscan.com/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 999,
        name: "HyperEVM",
        native_asset: "HYPE",
        explorer_url: "https://hyperevmscan.io/",
        tier: EtherscanTier::FreeUntil {
            year: 2026,
            month: 11,
            day: 30,
        },
    },
    EtherscanNetwork {
        chain_id: 1329,
        name: "Sei",
        native_asset: "SEI",
        explorer_url: "https://seitrace.com/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 988,
        name: "Stable",
        native_asset: "STABLE",
        explorer_url: "https://stablescan.xyz/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 9745,
        name: "Plasma",
        native_asset: "XPL",
        explorer_url: "https://plasmascan.to/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 747474,
        name: "Katana",
        native_asset: "ETH",
        explorer_url: "https://katanascan.com/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 4326,
        name: "MegaETH",
        native_asset: "ETH",
        explorer_url: "https://mega.etherscan.io/",
        tier: EtherscanTier::Free,
    },
    EtherscanNetwork {
        chain_id: 4663,
        name: "Robinhood Chain",
        native_asset: "ETH",
        explorer_url: "https://robin.etherscan.io/",
        tier: EtherscanTier::Paid,
    },
    EtherscanNetwork {
        chain_id: 5042,
        name: "Arc",
        native_asset: "USDC",
        explorer_url: "https://arc.etherscan.io/",
        tier: EtherscanTier::Paid,
    },
];

#[derive(Clone)]
pub struct EtherscanClient {
    client: Client,
    api_url: Url,
    api_key: Option<String>,
    paid_tier_enabled: bool,
    redis: Option<redis::Client>,
    last_request: Arc<Mutex<Option<Instant>>>,
}

#[derive(Debug, Error)]
pub enum EtherscanConfigError {
    #[error("ETHERSCAN_API_KEY contains invalid characters")]
    InvalidApiKey,
    #[error("ETHERSCAN_ENABLE_PAID_CHAINS must be true or false")]
    InvalidPaidTierSetting,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    status: String,
    message: Option<String>,
    result: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeTransaction {
    hash: String,
    block_number: String,
    time_stamp: String,
    from: String,
    to: String,
    value: String,
    is_error: Option<String>,
    #[serde(rename = "txreceipt_status")]
    txreceipt_status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InternalTransaction {
    hash: String,
    block_number: String,
    time_stamp: String,
    from: String,
    to: String,
    value: String,
    trace_id: String,
    is_error: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenTransfer {
    hash: String,
    block_number: String,
    time_stamp: String,
    from: String,
    to: String,
    value: String,
    contract_address: String,
    token_symbol: String,
    token_decimal: String,
    transaction_index: String,
}

impl EtherscanClient {
    pub fn from_env(client: Client, redis: redis::Client) -> Result<Self, EtherscanConfigError> {
        let api_key = env::var("ETHERSCAN_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty());
        let paid_tier_enabled = match env::var("ETHERSCAN_ENABLE_PAID_CHAINS") {
            Ok(value) if value.eq_ignore_ascii_case("true") || value == "1" => true,
            Ok(value)
                if value.eq_ignore_ascii_case("false")
                    || value == "0"
                    || value.trim().is_empty() =>
            {
                false
            }
            Ok(_) => return Err(EtherscanConfigError::InvalidPaidTierSetting),
            Err(_) => false,
        };
        Self::new_with_redis(api_key.as_deref(), paid_tier_enabled, client, Some(redis))
    }

    pub fn new(
        api_key: Option<&str>,
        paid_tier_enabled: bool,
        client: Client,
    ) -> Result<Self, EtherscanConfigError> {
        Self::new_with_redis(api_key, paid_tier_enabled, client, None)
    }

    fn new_with_redis(
        api_key: Option<&str>,
        paid_tier_enabled: bool,
        client: Client,
        redis: Option<redis::Client>,
    ) -> Result<Self, EtherscanConfigError> {
        if api_key.is_some_and(|key| key.chars().any(char::is_control)) {
            return Err(EtherscanConfigError::InvalidApiKey);
        }
        Ok(Self {
            client,
            api_url: Url::parse(API_URL).expect("provider URL constant is valid"),
            api_key: api_key.map(str::to_owned),
            paid_tier_enabled,
            redis,
            last_request: Arc::new(Mutex::new(None)),
        })
    }

    pub fn is_configured(&self) -> bool {
        self.api_key.is_some()
    }

    pub(crate) fn is_network_available_on(
        &self,
        network: EtherscanNetwork,
        as_of: NaiveDate,
    ) -> bool {
        self.is_configured()
            && (!network.tier.requires_paid_tier_on(as_of) || self.paid_tier_enabled)
    }

    async fn pace_request(&self) -> Result<(), ApiError> {
        let Some(redis) = &self.redis else {
            pace_request_local(&self.last_request).await;
            return Ok(());
        };

        loop {
            let mut connection = redis
                .get_multiplexed_async_connection()
                .await
                .map_err(|_| ApiError::ProviderUnavailable)?;
            let acquired: Option<String> = redis::cmd("SET")
                .arg(REDIS_REQUEST_GATE_KEY)
                .arg("1")
                .arg("NX")
                .arg("PX")
                .arg(MIN_REQUEST_INTERVAL.as_millis() as u64)
                .query_async(&mut connection)
                .await
                .map_err(|_| ApiError::ProviderUnavailable)?;
            drop(connection);
            if acquired.is_some() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    async fn fetch_pages<T: for<'de> Deserialize<'de>>(
        &self,
        chain_id: u64,
        address: &str,
        action: &str,
    ) -> Result<(Vec<T>, u32, bool), ApiError> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or(ApiError::ProviderUnavailable)?;
        let mut items = Vec::new();
        for page in 1..=MAX_PAGES {
            let request = self.client.get(self.api_url.clone()).query(&[
                ("chainid", chain_id.to_string()),
                ("module", "account".into()),
                ("action", action.into()),
                ("address", address.into()),
                ("startblock", "0".into()),
                ("endblock", "999999999".into()),
                ("page", page.to_string()),
                ("offset", PAGE_SIZE.to_string()),
                ("sort", "desc".into()),
                ("apikey", api_key.to_owned()),
            ]);
            self.pace_request().await?;
            let mut response = request
                .send()
                .await
                .map_err(|_| ApiError::ProviderUnavailable)?;
            if !response.status().is_success()
                || response
                    .content_length()
                    .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
            {
                return Err(ApiError::ProviderUnavailable);
            }
            let mut body = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| ApiError::ProviderUnavailable)?
            {
                if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                    return Err(ApiError::ProviderUnavailable);
                }
                body.extend_from_slice(&chunk);
            }
            let parsed: ApiResponse =
                serde_json::from_slice(&body).map_err(|_| ApiError::ProviderUnavailable)?;
            let page_items = match parsed.result {
                Value::Array(items) if parsed.status == "1" => items
                    .into_iter()
                    .map(serde_json::from_value)
                    .collect::<Result<Vec<T>, _>>()
                    .map_err(|_| ApiError::ProviderUnavailable)?,
                Value::String(message)
                    if parsed.status == "0"
                        && (message.eq_ignore_ascii_case("no transactions found")
                            || message.eq_ignore_ascii_case("no records found")) =>
                {
                    Vec::new()
                }
                _ => {
                    warn!(provider = "etherscan.io", action, message = ?parsed.message, "Etherscan API returned an error");
                    return Err(ApiError::ProviderUnavailable);
                }
            };
            let count = page_items.len();
            items.extend(page_items);
            if count < PAGE_SIZE as usize {
                return Ok((items, page, false));
            }
            if page == MAX_PAGES {
                return Ok((items, page, true));
            }
        }
        Ok((items, MAX_PAGES, true))
    }
}

async fn pace_request_local(last_request: &Mutex<Option<Instant>>) {
    let mut last = last_request.lock().await;
    if let Some(previous) = *last {
        tokio::time::sleep(MIN_REQUEST_INTERVAL.saturating_sub(previous.elapsed())).await;
    }
    *last = Some(Instant::now());
}

pub(crate) async fn fetch_history(
    client: &EtherscanClient,
    network: EtherscanNetwork,
    address: &str,
) -> Result<(Vec<EvmTransfer>, usize, bool), ApiError> {
    let (native, internal, tokens) = tokio::try_join!(
        client.fetch_pages::<NativeTransaction>(network.chain_id, address, "txlist"),
        client.fetch_pages::<InternalTransaction>(network.chain_id, address, "txlistinternal"),
        client.fetch_pages::<TokenTransfer>(network.chain_id, address, "tokentx"),
    )?;
    let mut transfers = Vec::with_capacity(native.0.len() + internal.0.len() + tokens.0.len());
    transfers.extend(
        native
            .0
            .into_iter()
            .filter_map(|item| normalize_native(item, address, network.native_asset)),
    );
    transfers.extend(
        internal
            .0
            .into_iter()
            .filter_map(|item| normalize_internal(item, address, network.native_asset)),
    );
    transfers.extend(normalize_tokens(tokens.0, address));
    Ok((
        transfers,
        (native.1 + internal.1 + tokens.1) as usize,
        native.2 || internal.2 || tokens.2,
    ))
}

fn normalize_native(item: NativeTransaction, tracked: &str, asset: &str) -> Option<EvmTransfer> {
    if item.is_error.as_deref() == Some("1")
        || item.txreceipt_status.as_deref() == Some("0")
        || !is_transaction_hash(&item.hash)
        || !is_evm_address(&item.from)
        || !is_evm_address(&item.to)
    {
        return None;
    }
    let amount = raw_amount(&item.value)?;
    if amount == "0" {
        return None;
    }
    let (direction, received, spent) = direction(&item.from, &item.to, tracked, &amount)?;
    let block_height = item.block_number.parse().ok();
    let block_time = unix_time(&item.time_stamp);
    Some(EvmTransfer {
        transaction_hash: item.hash.to_ascii_lowercase(),
        event_index: -1,
        transfer_kind: "evm_native",
        direction,
        received_amount: received,
        spent_amount: spent,
        asset: asset.into(),
        token_contract_address: None,
        token_decimals: None,
        from_address: Some(item.from.to_ascii_lowercase()),
        to_address: Some(item.to.to_ascii_lowercase()),
        block_height,
        block_time,
        confirmed: block_height.is_some() && block_time.is_some(),
    })
}

fn normalize_internal(
    item: InternalTransaction,
    tracked: &str,
    asset: &str,
) -> Option<EvmTransfer> {
    if item.is_error != "0"
        || !is_transaction_hash(&item.hash)
        || !is_evm_address(&item.from)
        || !is_evm_address(&item.to)
    {
        return None;
    }
    let amount = raw_amount(&item.value)?;
    if amount == "0" {
        return None;
    }
    let (direction, received, spent) = direction(&item.from, &item.to, tracked, &amount)?;
    let block_height = item.block_number.parse().ok();
    let block_time = unix_time(&item.time_stamp);
    Some(EvmTransfer {
        transaction_hash: item.hash.to_ascii_lowercase(),
        event_index: trace_index(&item.trace_id),
        transfer_kind: "evm_internal",
        direction,
        received_amount: received,
        spent_amount: spent,
        asset: asset.into(),
        token_contract_address: None,
        token_decimals: None,
        from_address: Some(item.from.to_ascii_lowercase()),
        to_address: Some(item.to.to_ascii_lowercase()),
        block_height,
        block_time,
        confirmed: block_height.is_some() && block_time.is_some(),
    })
}

fn normalize_tokens(items: Vec<TokenTransfer>, tracked: &str) -> Vec<EvmTransfer> {
    let mut occurrences = HashMap::<String, u64>::new();
    items
        .into_iter()
        .filter_map(|item| {
            if !is_transaction_hash(&item.hash)
                || !is_evm_address(&item.contract_address)
                || !is_evm_address(&item.from)
                || !is_evm_address(&item.to)
            {
                return None;
            }
            let amount = raw_amount(&item.value)?;
            let (direction, received, spent) = direction(&item.from, &item.to, tracked, &amount)?;
            let decimals = item
                .token_decimal
                .parse::<i16>()
                .ok()
                .filter(|value| (0..=255).contains(value));
            let symbol = sanitize_symbol(&item.token_symbol);
            let fingerprint = format!(
                "{}:{}:{}:{}:{}:{}",
                item.hash.to_ascii_lowercase(),
                item.contract_address.to_ascii_lowercase(),
                item.from.to_ascii_lowercase(),
                item.to.to_ascii_lowercase(),
                item.value,
                item.transaction_index
            );
            let occurrence = occurrences.entry(fingerprint.clone()).or_default();
            let event_index = stable_event_index(&fingerprint, *occurrence);
            *occurrence += 1;
            let block_height = item.block_number.parse().ok();
            let block_time = unix_time(&item.time_stamp);
            Some(EvmTransfer {
                transaction_hash: item.hash.to_ascii_lowercase(),
                event_index,
                transfer_kind: "evm_erc20",
                direction,
                received_amount: received,
                spent_amount: spent,
                asset: symbol.unwrap_or_else(|| "ERC-20".into()),
                token_contract_address: Some(item.contract_address.to_ascii_lowercase()),
                token_decimals: decimals,
                from_address: Some(item.from.to_ascii_lowercase()),
                to_address: Some(item.to.to_ascii_lowercase()),
                block_height,
                block_time,
                confirmed: block_height.is_some() && block_time.is_some(),
            })
        })
        .collect()
}

fn stable_event_index(value: &str, occurrence: u64) -> i64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.bytes().chain(occurrence.to_be_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    (hash & i64::MAX as u64) as i64
}

fn trace_index(value: &str) -> i64 {
    value
        .parse()
        .unwrap_or_else(|_| stable_event_index(value, 0))
}

fn direction(
    from: &str,
    to: &str,
    tracked: &str,
    amount: &str,
) -> Option<(&'static str, String, String)> {
    let outgoing = from.eq_ignore_ascii_case(tracked);
    let incoming = to.eq_ignore_ascii_case(tracked);
    match (incoming, outgoing) {
        (true, true) => Some(("both", amount.into(), amount.into())),
        (true, false) => Some(("inbound", amount.into(), "0".into())),
        (false, true) => Some(("outbound", "0".into(), amount.into())),
        (false, false) => None,
    }
}

fn raw_amount(value: &str) -> Option<String> {
    (!value.is_empty() && value.len() <= 78 && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.to_owned())
}

fn unix_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(value.parse().ok()?, 0)
}

fn sanitize_symbol(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.chars().count() <= 20 && value.chars().all(|ch| !ch.is_control()))
        .then(|| value.to_owned())
}

fn is_transaction_hash(value: &str) -> bool {
    value.len() == 66
        && value.starts_with("0x")
        && value[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_evm_address(value: &str) -> bool {
    value.len() == 42
        && value.starts_with("0x")
        && value[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc, time::Duration};

    use axum::{Router, extract::Query, http::StatusCode, routing::get};
    use chrono::NaiveDate;
    use reqwest::Url;
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::Mutex, time::Instant};

    use crate::error::ApiError;

    use super::{
        EtherscanClient, EtherscanTier, REDIS_REQUEST_GATE_KEY, SUPPORTED_NETWORKS, TokenTransfer,
        normalize_internal, normalize_native, normalize_tokens, pace_request_local, trace_index,
    };

    const ADDRESS: &str = "0x0123456789abcdef0123456789abcdef01234567";
    const COUNTERPARTY: &str = "0xabcdef0123456789abcdef0123456789abcdef01";
    const HASH: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn maps_native_and_internal_transfers_and_skips_failed_events() {
        let native: super::NativeTransaction = serde_json::from_value(json!({
            "hash": HASH,
            "blockNumber": "123",
            "timeStamp": "1700000000",
            "from": COUNTERPARTY,
            "to": ADDRESS,
            "value": "1000000000000000000",
            "isError": "0",
            "txreceipt_status": "1"
        }))
        .unwrap();
        let transfer = normalize_native(native, ADDRESS, "BNB").unwrap();
        assert_eq!(transfer.transfer_kind, "evm_native");
        assert_eq!(transfer.direction, "inbound");
        assert_eq!(transfer.received_amount, "1000000000000000000");
        assert_eq!(transfer.asset, "BNB");
        assert!(transfer.confirmed);

        let failed: super::NativeTransaction = serde_json::from_value(json!({
            "hash": HASH, "blockNumber": "123", "timeStamp": "1700000000",
            "from": COUNTERPARTY, "to": ADDRESS, "value": "2", "isError": "1"
        }))
        .unwrap();
        assert!(normalize_native(failed, ADDRESS, "BNB").is_none());

        let internal: super::InternalTransaction = serde_json::from_value(json!({
            "hash": HASH, "blockNumber": "123", "timeStamp": "1700000000",
            "from": ADDRESS, "to": COUNTERPARTY, "value": "7", "traceId": "0_1", "isError": "0"
        }))
        .unwrap();
        let internal = normalize_internal(internal, ADDRESS, "BNB").unwrap();
        assert_eq!(internal.transfer_kind, "evm_internal");
        assert_eq!(internal.direction, "outbound");
        assert!(internal.event_index >= 0);
        assert_eq!(trace_index("0_1"), trace_index("0_1"));
        assert_ne!(trace_index("0_1"), trace_index("0_2"));
    }

    #[test]
    fn maps_erc20_values_and_assigns_distinct_stable_event_indexes() {
        let row = || TokenTransfer {
            hash: HASH.into(),
            block_number: "123".into(),
            time_stamp: "1700000000".into(),
            from: COUNTERPARTY.into(),
            to: ADDRESS.into(),
            value: "9".into(),
            contract_address: "0x9999999999999999999999999999999999999999".into(),
            token_symbol: "USDT".into(),
            token_decimal: "6".into(),
            transaction_index: "4".into(),
        };
        let first = normalize_tokens(vec![row(), row()], ADDRESS);
        let second = normalize_tokens(vec![row(), row()], ADDRESS);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].transfer_kind, "evm_erc20");
        assert_eq!(first[0].token_decimals, Some(6));
        assert_eq!(first[0].received_amount, "9");
        assert_ne!(first[0].event_index, first[1].event_index);
        assert_eq!(first[0].event_index, second[0].event_index);
        assert_eq!(first[1].event_index, second[1].event_index);
    }

    #[test]
    fn allows_only_documented_mainnet_chain_ids_and_tier_metadata() {
        assert!(
            SUPPORTED_NETWORKS
                .iter()
                .any(|network| { network.chain_id == 56 && network.tier == EtherscanTier::Paid })
        );
        assert!(
            SUPPORTED_NETWORKS
                .iter()
                .any(|network| { network.chain_id == 137 && network.tier == EtherscanTier::Free })
        );
        assert!(
            SUPPORTED_NETWORKS
                .iter()
                .any(|network| { network.chain_id == 10 && network.tier == EtherscanTier::Paid })
        );
        let mut chain_ids = SUPPORTED_NETWORKS
            .iter()
            .map(|network| network.chain_id)
            .collect::<Vec<_>>();
        chain_ids.sort_unstable();
        chain_ids.dedup();
        assert_eq!(chain_ids.len(), SUPPORTED_NETWORKS.len());
        assert!(
            SUPPORTED_NETWORKS
                .iter()
                .all(|network| network.explorer_url.starts_with("https://"))
        );
    }

    #[test]
    fn hyper_evm_requires_paid_opt_in_after_the_published_free_period() {
        let hyper_evm = *SUPPORTED_NETWORKS
            .iter()
            .find(|network| network.chain_id == 999)
            .unwrap();
        let before_cutoff = NaiveDate::from_ymd_opt(2026, 11, 30).unwrap();
        let after_cutoff = NaiveDate::from_ymd_opt(2026, 12, 1).unwrap();
        let free_client =
            EtherscanClient::new(Some("test-key"), false, reqwest::Client::new()).unwrap();
        let paid_client =
            EtherscanClient::new(Some("test-key"), true, reqwest::Client::new()).unwrap();

        assert!(!hyper_evm.tier.requires_paid_tier_on(before_cutoff));
        assert!(free_client.is_network_available_on(hyper_evm, before_cutoff));
        assert!(hyper_evm.tier.requires_paid_tier_on(after_cutoff));
        assert!(!free_client.is_network_available_on(hyper_evm, after_cutoff));
        assert!(paid_client.is_network_available_on(hyper_evm, after_cutoff));
    }

    #[tokio::test]
    async fn sends_v2_query_and_accepts_empty_history_response() {
        let seen = Arc::new(Mutex::new(None::<HashMap<String, String>>));
        let handler_seen = seen.clone();
        let app = Router::new().route(
            "/v2/api",
            get(move |Query(query): Query<HashMap<String, String>>| {
                let seen = handler_seen.clone();
                async move {
                    *seen.lock().await = Some(query);
                    axum::Json(json!({
                        "status": "0",
                        "message": "No transactions found",
                        "result": "No transactions found"
                    }))
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let mut client =
            EtherscanClient::new(Some("test-key"), false, reqwest::Client::new()).unwrap();
        client.api_url = Url::parse(&format!("http://{address}/v2/api")).unwrap();
        let (items, pages, more) = client
            .fetch_pages::<Value>(56, ADDRESS, "txlist")
            .await
            .unwrap();
        assert!(items.is_empty());
        assert_eq!(pages, 1);
        assert!(!more);
        let query = seen.lock().await.clone().unwrap();
        assert_eq!(query.get("chainid").map(String::as_str), Some("56"));
        assert_eq!(query.get("action").map(String::as_str), Some("txlist"));
        assert_eq!(query.get("address").map(String::as_str), Some(ADDRESS));
        assert_eq!(query.get("apikey").map(String::as_str), Some("test-key"));
        server.abort();
    }

    #[tokio::test]
    async fn maps_provider_http_errors_to_provider_unavailable() {
        let app = Router::new().route("/v2/api", get(|| async { StatusCode::TOO_MANY_REQUESTS }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let mut client =
            EtherscanClient::new(Some("test-key"), false, reqwest::Client::new()).unwrap();
        client.api_url = Url::parse(&format!("http://{address}/v2/api")).unwrap();
        assert!(
            client
                .fetch_pages::<Value>(56, ADDRESS, "txlist")
                .await
                .is_err()
        );
        server.abort();
    }

    #[tokio::test]
    async fn spaces_concurrent_requests_to_the_free_tier_limit() {
        let gate = Mutex::new(None);
        pace_request_local(&gate).await;
        let first = Instant::now();
        pace_request_local(&gate).await;
        assert!(first.elapsed() >= Duration::from_millis(350));
    }

    #[tokio::test]
    async fn redis_gate_serializes_separate_clients() {
        let redis_url = std::env::var("REDIS_URL").expect("CI supplies Redis for this test");
        let redis = redis::Client::open(redis_url).unwrap();
        let mut connection = redis.get_multiplexed_async_connection().await.unwrap();
        let _: i32 = redis::cmd("DEL")
            .arg(REDIS_REQUEST_GATE_KEY)
            .query_async(&mut connection)
            .await
            .unwrap();
        drop(connection);

        let first = EtherscanClient::new_with_redis(
            Some("test-key"),
            false,
            reqwest::Client::new(),
            Some(redis.clone()),
        )
        .unwrap();
        let second = EtherscanClient::new_with_redis(
            Some("test-key"),
            false,
            reqwest::Client::new(),
            Some(redis.clone()),
        )
        .unwrap();
        first.pace_request().await.unwrap();
        let started = Instant::now();
        second.pace_request().await.unwrap();
        assert!(started.elapsed() >= Duration::from_millis(350));
        let mut connection = redis.get_multiplexed_async_connection().await.unwrap();
        let _: i32 = redis::cmd("DEL")
            .arg(REDIS_REQUEST_GATE_KEY)
            .query_async(&mut connection)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn redis_gate_fails_closed_when_redis_is_unavailable() {
        let redis = redis::Client::open("redis://127.0.0.1:1/").unwrap();
        let client = EtherscanClient::new_with_redis(
            Some("test-key"),
            false,
            reqwest::Client::new(),
            Some(redis),
        )
        .unwrap();
        assert!(matches!(
            client.pace_request().await,
            Err(ApiError::ProviderUnavailable)
        ));
    }
}
