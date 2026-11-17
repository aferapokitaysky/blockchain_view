use std::{
    collections::{BTreeMap, HashSet},
    env,
    net::IpAddr,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::http::HeaderValue;
use chrono::{DateTime, Utc};
use reqwest::{Client, Url};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::warn;

use crate::{
    ApiState,
    error::ApiError,
    networks::{self, NetworkInfo},
};

const MAX_PAGES: usize = 10;
const ITEMS_PER_PAGE: &str = "50";
const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024;
const NETWORK_CATALOG_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const NETWORK_CATALOG_RETRY_DELAY: Duration = Duration::from_millis(200);
const NETWORK_CATALOG_FAILURE_TTL: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct BlockscoutClient {
    client: Client,
    base_url: Url,
    api_key: Option<HeaderValue>,
    prefer_configured_base: bool,
    catalog_cache: Arc<RwLock<Option<CachedNetworkCatalog>>>,
}

#[derive(Clone)]
struct CachedNetworkCatalog {
    expires_at: Instant,
    networks: Vec<NetworkInfo>,
}

#[derive(Debug, Error)]
pub enum BlockscoutConfigError {
    #[error("BLOCKSCOUT_BASE_URL is invalid")]
    InvalidBaseUrl,
    #[error("BLOCKSCOUT_BASE_URL must use HTTPS outside loopback development")]
    InsecureBaseUrl,
    #[error("BLOCKSCOUT_API_KEY contains invalid header characters")]
    InvalidApiKey,
}

impl BlockscoutClient {
    pub fn from_env(client: Client) -> Result<Self, BlockscoutConfigError> {
        let configured_base_url = env::var("BLOCKSCOUT_BASE_URL").ok();
        let base_url = configured_base_url
            .clone()
            .unwrap_or_else(|| "https://api.blockscout.com".into());
        let api_key = env::var("BLOCKSCOUT_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let mut provider = Self::new(&base_url, api_key.as_deref(), client)?;
        provider.prefer_configured_base =
            prefers_configured_base(&provider.base_url, provider.api_key.is_some());
        Ok(provider)
    }

    pub fn new(
        base_url: &str,
        api_key: Option<&str>,
        client: Client,
    ) -> Result<Self, BlockscoutConfigError> {
        let base_url = Url::parse(base_url).map_err(|_| BlockscoutConfigError::InvalidBaseUrl)?;
        if !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
            || base_url.path() != "/"
        {
            return Err(BlockscoutConfigError::InvalidBaseUrl);
        }

        let secure = base_url.scheme() == "https";
        let loopback_http =
            base_url.scheme() == "http" && base_url.host_str().is_some_and(is_loopback_host);
        if !secure && !loopback_http {
            return Err(BlockscoutConfigError::InsecureBaseUrl);
        }

        let api_key = api_key
            .map(|key| HeaderValue::from_str(key).map_err(|_| BlockscoutConfigError::InvalidApiKey))
            .transpose()?;

        Ok(Self {
            client,
            base_url,
            api_key,
            prefer_configured_base: true,
            catalog_cache: Arc::new(RwLock::new(None)),
        })
    }

    fn source_host_for(&self, chain: &EvmChain) -> String {
        self.provider_base_url(chain)
            .host_str()
            .unwrap_or("unknown")
            .to_owned()
    }

    fn provider_base_url<'a>(&'a self, chain: &'a EvmChain) -> &'a Url {
        if self.prefer_configured_base {
            &self.base_url
        } else {
            chain.api_base_url().unwrap_or(&self.base_url)
        }
    }

    pub(crate) async fn network_catalog(&self) -> Vec<NetworkInfo> {
        if let Some(cached) = self.catalog_cache.read().await.as_ref()
            && cached.expires_at > Instant::now()
        {
            return cached.networks.clone();
        }

        let mut cache = self.catalog_cache.write().await;
        if let Some(cached) = cache.as_ref()
            && cached.expires_at > Instant::now()
        {
            return cached.networks.clone();
        }
        match self.fetch_network_catalog().await {
            Ok(networks) => {
                *cache = Some(CachedNetworkCatalog {
                    expires_at: Instant::now() + NETWORK_CATALOG_TTL,
                    networks: networks.clone(),
                });
                return networks;
            }
            Err(_) => {
                warn!(provider = %self.base_url.host_str().unwrap_or("unknown"), "Blockscout network catalog refresh failed");
                if let Some(cached) = cache.as_mut() {
                    cached.expires_at = Instant::now() + NETWORK_CATALOG_FAILURE_TTL;
                } else {
                    *cache = Some(CachedNetworkCatalog {
                        expires_at: Instant::now() + NETWORK_CATALOG_FAILURE_TTL,
                        networks: networks::fallback_catalog(),
                    });
                }
            }
        }

        cache
            .as_ref()
            .map(|cached| cached.networks.clone())
            .unwrap_or_else(networks::fallback_catalog)
    }

    async fn fetch_network_catalog(&self) -> Result<Vec<NetworkInfo>, ApiError> {
        let url = self
            .base_url
            .join("/api/json/config")
            .map_err(|_| ApiError::ProviderUnavailable)?;
        let mut response = None;
        for attempt in 0..2 {
            match self.client.get(url.clone()).send().await {
                Ok(result) if attempt == 0 && result.status().is_server_error() => {
                    tokio::time::sleep(NETWORK_CATALOG_RETRY_DELAY).await;
                }
                Ok(result) => {
                    response = Some(result);
                    break;
                }
                Err(_) if attempt == 0 => {
                    tokio::time::sleep(NETWORK_CATALOG_RETRY_DELAY).await;
                }
                Err(_) => return Err(ApiError::ProviderUnavailable),
            }
        }
        let response = response.ok_or(ApiError::ProviderUnavailable)?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > networks::MAX_CATALOG_BYTES as u64)
        {
            return Err(ApiError::ProviderUnavailable);
        }
        let mut response = response;
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ApiError::ProviderUnavailable)?
        {
            if body.len() + chunk.len() > networks::MAX_CATALOG_BYTES {
                return Err(ApiError::ProviderUnavailable);
            }
            body.extend_from_slice(&chunk);
        }
        let configured_universal_provider = self.api_key.is_some()
            || (self.prefer_configured_base
                && self.base_url.host_str() != Some("api.blockscout.com"));
        networks::parse_blockscout_config(&body, configured_universal_provider)
            .map_err(|_| ApiError::ProviderUnavailable)
    }

    async fn fetch_pages<T: DeserializeOwned>(
        &self,
        chain: &EvmChain,
        address: &str,
        endpoint: &str,
        extra_params: &[(&str, &str)],
    ) -> Result<FetchedPages<T>, ApiError> {
        self.fetch_pages_bounded(chain, address, endpoint, extra_params, MAX_PAGES)
            .await
    }

    async fn fetch_pages_bounded<T: DeserializeOwned>(
        &self,
        chain: &EvmChain,
        address: &str,
        endpoint: &str,
        extra_params: &[(&str, &str)],
        max_pages: usize,
    ) -> Result<FetchedPages<T>, ApiError> {
        let base_url = self.provider_base_url(chain);
        let universal_endpoint = self.prefer_configured_base || chain.api_base_url().is_none();
        let source_host = base_url.host_str().unwrap_or("unknown").to_owned();
        let url = history_endpoint_url(
            base_url,
            chain.chain_id(),
            address,
            endpoint,
            universal_endpoint,
        )?;
        let host = source_host;
        let mut cursor: Option<BTreeMap<String, Value>> = None;
        let mut seen_cursors = HashSet::new();
        let mut items = Vec::new();
        let mut pages = 0;

        loop {
            let mut request = self
                .client
                .get(url.clone())
                .query(&[("items_count", ITEMS_PER_PAGE)]);
            for (key, value) in extra_params {
                request = request.query(&[(key, value)]);
            }
            if let Some(page_cursor) = &cursor {
                for (key, value) in page_cursor {
                    if !value.is_null() {
                        let value = value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string());
                        request = request.query(&[(key, value)]);
                    }
                }
            }
            if let Some(api_key) = &self.api_key {
                let mut authorization = b"Bearer ".to_vec();
                authorization.extend_from_slice(api_key.as_bytes());
                let authorization = HeaderValue::from_bytes(&authorization)
                    .map_err(|_| ApiError::ProviderUnavailable)?;
                request = request.header(reqwest::header::AUTHORIZATION, authorization);
            }

            let response = request.send().await.map_err(|_| {
                warn!(provider = %host, "Blockscout request failed");
                ApiError::ProviderUnavailable
            })?;
            if !response.status().is_success() {
                warn!(provider = %host, status = %response.status(), "Blockscout returned an error status");
                return Err(ApiError::ProviderUnavailable);
            }
            if response
                .content_length()
                .is_some_and(|length| length > MAX_PAGE_BYTES as u64)
            {
                warn!(provider = %host, "Blockscout page exceeded the response size limit");
                return Err(ApiError::ProviderUnavailable);
            }
            let mut response = response;
            let mut body = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| ApiError::ProviderUnavailable)?
            {
                if body.len() + chunk.len() > MAX_PAGE_BYTES {
                    warn!(provider = %host, "Blockscout page exceeded the response size limit");
                    return Err(ApiError::ProviderUnavailable);
                }
                body.extend_from_slice(&chunk);
            }
            let page: BlockscoutPage<T> = serde_json::from_slice(&body).map_err(|_| {
                warn!(provider = %host, "Blockscout returned an invalid page");
                ApiError::ProviderUnavailable
            })?;
            pages += 1;
            items.extend(page.items);
            cursor = page.next_page_params;

            let Some(next_cursor) = &cursor else {
                break;
            };
            if next_cursor.is_empty() {
                return Err(ApiError::ProviderUnavailable);
            }
            let cursor_key =
                serde_json::to_string(next_cursor).map_err(|_| ApiError::ProviderUnavailable)?;
            if !seen_cursors.insert(cursor_key) {
                warn!(provider = %host, "Blockscout repeated a pagination cursor");
                return Err(ApiError::ProviderUnavailable);
            }
            if pages >= max_pages {
                break;
            }
        }

        Ok(FetchedPages {
            items,
            pages,
            may_have_more: cursor.is_some(),
        })
    }
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || IpAddr::from_str(host).is_ok_and(|address| address.is_loopback())
}

fn prefers_configured_base(base_url: &Url, has_api_key: bool) -> bool {
    let is_public_aggregator = base_url.host_str() == Some("api.blockscout.com")
        && base_url.scheme() == "https"
        && base_url.path() == "/";
    !is_public_aggregator || has_api_key
}

fn history_endpoint_url(
    base_url: &Url,
    chain_id: u64,
    address: &str,
    endpoint: &str,
    universal_endpoint: bool,
) -> Result<Url, ApiError> {
    let path = if universal_endpoint {
        format!("/{chain_id}/api/v2/addresses/{address}/{endpoint}")
    } else {
        format!("/api/v2/addresses/{address}/{endpoint}")
    };
    base_url
        .join(&path)
        .map_err(|_| ApiError::ProviderUnavailable)
}

#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct BlockscoutPage<T> {
    #[serde(default)]
    items: Vec<T>,
    next_page_params: Option<BTreeMap<String, Value>>,
}

pub(crate) struct FetchedPages<T> {
    pub items: Vec<T>,
    pub pages: usize,
    pub may_have_more: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct EvmChain {
    chain_id: u64,
    native_asset: String,
    api_base_url: Option<Url>,
}

impl EvmChain {
    fn new(chain_id: u64, native_asset: impl Into<String>, api_base_url: Option<Url>) -> Self {
        Self {
            chain_id,
            native_asset: native_asset.into(),
            api_base_url,
        }
    }

    fn chain_id(&self) -> u64 {
        self.chain_id
    }

    fn native_asset(&self) -> &str {
        &self.native_asset
    }

    fn api_base_url(&self) -> Option<&Url> {
        self.api_base_url.as_ref()
    }

    fn from_network(network: NetworkInfo) -> Option<Self> {
        if network.family != networks::NetworkFamily::Evm {
            return None;
        }
        let chain_id = network.chain_id?;
        Some(Self::new(
            chain_id,
            network.native_asset,
            network.api_base_url,
        ))
    }
}

#[derive(Debug, Deserialize)]
struct AddressRef {
    hash: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NativeTransaction {
    hash: String,
    from: Option<AddressRef>,
    to: Option<AddressRef>,
    value: String,
    status: Option<String>,
    block_number: Option<i64>,
    timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct TokenTransfer {
    transaction_hash: String,
    log_index: i64,
    from: Option<AddressRef>,
    to: Option<AddressRef>,
    timestamp: Option<DateTime<Utc>>,
    block_number: Option<i64>,
    token: TokenMetadata,
    total: Option<TokenTotal>,
    token_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InternalTransaction {
    transaction_hash: String,
    index: i64,
    from: Option<AddressRef>,
    to: Option<AddressRef>,
    value: String,
    success: bool,
    block_number: Option<i64>,
    timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct TokenMetadata {
    address_hash: Option<String>,
    decimals: Option<String>,
    symbol: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenTotal {
    value: Option<String>,
    token_instance: Option<TokenInstance>,
}

#[derive(Debug, Deserialize)]
struct TokenInstance {
    value: Option<String>,
}

#[derive(Debug)]
pub(crate) struct EvmTransfer {
    pub transaction_hash: String,
    pub event_index: i64,
    pub transfer_kind: &'static str,
    pub direction: &'static str,
    pub received_amount: String,
    pub spent_amount: String,
    pub asset: String,
    pub token_contract_address: Option<String>,
    pub token_decimals: Option<i16>,
    pub from_address: Option<String>,
    pub to_address: Option<String>,
    pub block_height: Option<i64>,
    pub block_time: Option<DateTime<Utc>>,
    pub confirmed: bool,
}

pub(crate) async fn fetch_history(
    provider: &BlockscoutClient,
    chain: EvmChain,
    address: &str,
) -> Result<(Vec<EvmTransfer>, usize, bool), ApiError> {
    let (transactions, internal_transactions, token_transfers) = tokio::try_join!(
        provider.fetch_pages::<NativeTransaction>(&chain, address, "transactions", &[]),
        provider.fetch_pages::<InternalTransaction>(&chain, address, "internal-transactions", &[]),
        provider.fetch_pages::<TokenTransfer>(
            &chain,
            address,
            "token-transfers",
            &[("type", "ERC-20")],
        )
    )?;

    let mut transfers = Vec::new();
    for transaction in &transactions.items {
        if let Some(transfer) = normalize_native_transaction(transaction, address, &chain) {
            transfers.push(transfer);
        }
    }
    for internal_transaction in &internal_transactions.items {
        if let Some(transfer) =
            normalize_internal_transaction(internal_transaction, address, &chain)
        {
            transfers.push(transfer);
        }
    }
    for token_transfer in &token_transfers.items {
        if let Some(transfer) = normalize_token_transfer(token_transfer, address) {
            transfers.push(transfer);
        }
    }

    Ok((
        transfers,
        transactions.pages + internal_transactions.pages + token_transfers.pages,
        transactions.may_have_more
            || internal_transactions.may_have_more
            || token_transfers.may_have_more,
    ))
}

fn normalize_internal_transaction(
    transaction: &InternalTransaction,
    tracked_address: &str,
    chain: &EvmChain,
) -> Option<EvmTransfer> {
    if !transaction.success
        || transaction.index < 0
        || !is_transaction_hash(&transaction.transaction_hash)
    {
        return None;
    }
    let amount = parse_raw_amount(&transaction.value)?;
    if amount == "0" {
        return None;
    }
    let from = transaction
        .from
        .as_ref()
        .and_then(|address| address.hash.as_deref());
    let to = transaction
        .to
        .as_ref()
        .and_then(|address| address.hash.as_deref());
    let (direction, received, spent) = transfer_direction(from, to, tracked_address, &amount)?;

    Some(EvmTransfer {
        transaction_hash: transaction.transaction_hash.to_ascii_lowercase(),
        event_index: transaction.index,
        transfer_kind: "evm_internal",
        direction,
        received_amount: received,
        spent_amount: spent,
        asset: chain.native_asset().to_owned(),
        token_contract_address: None,
        token_decimals: None,
        from_address: from.map(str::to_ascii_lowercase),
        to_address: to.map(str::to_ascii_lowercase),
        block_height: transaction.block_number.filter(|height| *height >= 0),
        block_time: transaction.timestamp,
        confirmed: transaction.block_number.is_some() && transaction.timestamp.is_some(),
    })
}

fn normalize_native_transaction(
    transaction: &NativeTransaction,
    tracked_address: &str,
    chain: &EvmChain,
) -> Option<EvmTransfer> {
    if transaction.status.as_deref() == Some("error") || !is_transaction_hash(&transaction.hash) {
        return None;
    }
    let amount = parse_raw_amount(&transaction.value)?;
    if amount == "0" {
        return None;
    }
    let from = transaction
        .from
        .as_ref()
        .and_then(|address| address.hash.as_deref());
    let to = transaction
        .to
        .as_ref()
        .and_then(|address| address.hash.as_deref());
    let (direction, received, spent) = transfer_direction(from, to, tracked_address, &amount)?;

    Some(EvmTransfer {
        transaction_hash: transaction.hash.to_ascii_lowercase(),
        event_index: -1,
        transfer_kind: "evm_native",
        direction,
        received_amount: received,
        spent_amount: spent,
        asset: chain.native_asset().to_owned(),
        token_contract_address: None,
        token_decimals: None,
        from_address: from.map(str::to_ascii_lowercase),
        to_address: to.map(str::to_ascii_lowercase),
        block_height: transaction.block_number.filter(|height| *height >= 0),
        block_time: transaction.timestamp,
        confirmed: transaction.block_number.is_some() && transaction.timestamp.is_some(),
    })
}

fn normalize_token_transfer(
    transfer: &TokenTransfer,
    tracked_address: &str,
) -> Option<EvmTransfer> {
    if transfer.token_type.as_deref() != Some("ERC-20")
        || transfer.log_index < 0
        || !is_transaction_hash(&transfer.transaction_hash)
    {
        return None;
    }
    let amount = transfer
        .total
        .as_ref()
        .and_then(|total| {
            total
                .value
                .as_ref()
                .or_else(|| total.token_instance.as_ref()?.value.as_ref())
        })
        .and_then(|value| parse_raw_amount(value))?;
    let contract = transfer.token.address_hash.as_deref()?.to_ascii_lowercase();
    if !is_evm_address(&contract) {
        return None;
    }
    let from = transfer
        .from
        .as_ref()
        .and_then(|address| address.hash.as_deref());
    let to = transfer
        .to
        .as_ref()
        .and_then(|address| address.hash.as_deref());
    let (direction, received, spent) = transfer_direction(from, to, tracked_address, &amount)?;
    let decimals = transfer
        .token
        .decimals
        .as_deref()
        .and_then(|value| value.parse::<i16>().ok())
        .filter(|value| (0..=255).contains(value));
    let asset = transfer
        .token
        .symbol
        .as_deref()
        .map(str::trim)
        .filter(|symbol| !symbol.is_empty())
        .unwrap_or("ERC-20");

    Some(EvmTransfer {
        transaction_hash: transfer.transaction_hash.to_ascii_lowercase(),
        event_index: transfer.log_index,
        transfer_kind: "erc20",
        direction,
        received_amount: received,
        spent_amount: spent,
        asset: asset.to_owned(),
        token_contract_address: Some(contract),
        token_decimals: decimals,
        from_address: from.map(str::to_ascii_lowercase),
        to_address: to.map(str::to_ascii_lowercase),
        block_height: transfer.block_number.filter(|height| *height >= 0),
        block_time: transfer.timestamp,
        confirmed: transfer.block_number.is_some() && transfer.timestamp.is_some(),
    })
}

fn transfer_direction(
    from: Option<&str>,
    to: Option<&str>,
    tracked_address: &str,
    amount: &str,
) -> Option<(&'static str, String, String)> {
    let outgoing = from.is_some_and(|address| address.eq_ignore_ascii_case(tracked_address));
    let incoming = to.is_some_and(|address| address.eq_ignore_ascii_case(tracked_address));
    match (incoming, outgoing) {
        (true, true) => Some(("both", amount.to_owned(), amount.to_owned())),
        (true, false) => Some(("inbound", amount.to_owned(), "0".into())),
        (false, true) => Some(("outbound", "0".into(), amount.to_owned())),
        (false, false) => None,
    }
}

fn parse_raw_amount(value: &str) -> Option<String> {
    let value = value.trim();
    (value.len() <= 78 && !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
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

pub(crate) async fn sync_evm_address(
    state: &ApiState,
    tracked_address_id: uuid::Uuid,
    network: &str,
    address: &str,
) -> Result<super::transfers::SyncResult, ApiError> {
    let network_info = networks::provider_catalog(state)
        .await
        .into_iter()
        .find(|candidate| candidate.id == network)
        .ok_or(ApiError::UnsupportedNetwork)?;
    if !network_info.history_sync_available {
        return Err(ApiError::ProviderUnavailable);
    }
    let (transfers, pages, may_have_more_history, provider_name, source_host) = match network_info
        .sync_provider
    {
        Some(networks::NetworkSyncProvider::Blockscout) => {
            let chain = EvmChain::from_network(network_info).ok_or(ApiError::UnsupportedNetwork)?;
            let (transfers, pages, more) =
                fetch_history(&state.blockscout, chain.clone(), address).await?;
            let source_host = state.blockscout.source_host_for(&chain);
            let provider_name = format!("Blockscout ({source_host})");
            (transfers, pages, more, provider_name, source_host)
        }
        Some(networks::NetworkSyncProvider::EtherscanV2) => {
            let chain_id = network_info.chain_id.ok_or(ApiError::UnsupportedNetwork)?;
            let supported = crate::etherscan_sync::SUPPORTED_NETWORKS
                .iter()
                .copied()
                .find(|candidate| candidate.chain_id == chain_id)
                .ok_or(ApiError::UnsupportedNetwork)?;
            let (transfers, pages, more) =
                crate::etherscan_sync::fetch_history(&state.etherscan, supported, address).await?;
            (
                transfers,
                pages,
                more,
                "Etherscan API V2".to_owned(),
                "api.etherscan.io".to_owned(),
            )
        }
        None => return Err(ApiError::ProviderUnavailable),
    };
    let observed_at = Utc::now();
    let mut transaction = state.database.begin().await?;
    let mut saved = 0;

    for transfer in &transfers {
        let result = sqlx::query(
            "INSERT INTO observed_transfers \
             (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, received_amount, spent_amount, asset, token_contract_address, token_decimals, from_address, to_address, block_height, block_time, confirmed, provider, source_host, observed_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19) \
             ON CONFLICT (tracked_address_id, transaction_hash, transfer_kind, event_index) DO UPDATE SET \
             transfer_kind = EXCLUDED.transfer_kind, direction = EXCLUDED.direction, received_amount = EXCLUDED.received_amount, spent_amount = EXCLUDED.spent_amount, asset = EXCLUDED.asset, token_contract_address = EXCLUDED.token_contract_address, token_decimals = EXCLUDED.token_decimals, from_address = EXCLUDED.from_address, to_address = EXCLUDED.to_address, block_height = EXCLUDED.block_height, block_time = EXCLUDED.block_time, confirmed = EXCLUDED.confirmed, provider = EXCLUDED.provider, source_host = EXCLUDED.source_host, observed_at = EXCLUDED.observed_at",
        )
        .bind(tracked_address_id)
        .bind(network)
        .bind(&transfer.transaction_hash)
        .bind(transfer.event_index)
        .bind(transfer.transfer_kind)
        .bind(transfer.direction)
        .bind(&transfer.received_amount)
        .bind(&transfer.spent_amount)
        .bind(&transfer.asset)
        .bind(&transfer.token_contract_address)
        .bind(transfer.token_decimals)
        .bind(&transfer.from_address)
        .bind(&transfer.to_address)
        .bind(transfer.block_height)
        .bind(transfer.block_time)
        .bind(transfer.confirmed)
        .bind(&provider_name)
        .bind(&source_host)
        .bind(observed_at)
        .execute(&mut *transaction)
        .await?;
        saved += result.rows_affected() as usize;
    }
    transaction.commit().await?;

    Ok(super::transfers::SyncResult {
        provider: provider_name,
        fetched: transfers.len(),
        saved,
        pages,
        may_have_more_history,
        observed_at,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, VecDeque},
        sync::{Arc, Mutex},
    };

    use axum::{
        Router,
        body::Body,
        extract::State,
        http::{HeaderMap, Response, StatusCode, Uri},
        routing::get,
    };
    use reqwest::Client;
    use tokio::task::JoinHandle;

    use super::{
        BlockscoutClient, EvmChain, InternalTransaction, NativeTransaction, TokenTransfer,
        history_endpoint_url, normalize_internal_transaction, normalize_native_transaction,
        normalize_token_transfer, prefers_configured_base,
    };
    use crate::error::ApiError;

    const TRACKED: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn maps_supported_networks_to_explicit_chain_ids() {
        assert_eq!(EvmChain::new(1, "ETH", None).chain_id(), 1);
        assert_eq!(EvmChain::new(8453, "ETH", None).chain_id(), 8453);
        assert_eq!(EvmChain::new(42161, "ETH", None).chain_id(), 42161);
    }

    #[test]
    fn builds_paths_for_universal_and_direct_blockscout_apis() {
        let base = reqwest::Url::parse("https://api.blockscout.com/").unwrap();
        let direct = reqwest::Url::parse("https://eth.blockscout.com/").unwrap();

        assert_eq!(
            history_endpoint_url(&base, 10, TRACKED, "transactions", true)
                .unwrap()
                .as_str(),
            format!("https://api.blockscout.com/10/api/v2/addresses/{TRACKED}/transactions")
        );
        assert_eq!(
            history_endpoint_url(&direct, 1, TRACKED, "transactions", false)
                .unwrap()
                .as_str(),
            format!("https://eth.blockscout.com/api/v2/addresses/{TRACKED}/transactions")
        );
    }

    #[test]
    fn uses_public_instances_without_a_key_and_multichain_api_with_a_key() {
        let aggregator = reqwest::Url::parse("https://api.blockscout.com/").unwrap();
        let custom = reqwest::Url::parse("https://blockscout.example.org/").unwrap();
        assert!(!prefers_configured_base(&aggregator, false));
        assert!(prefers_configured_base(&aggregator, true));
        assert!(prefers_configured_base(&custom, false));
    }

    #[test]
    fn parses_native_value_as_raw_wei_and_sets_direction() {
        let fixture = include_str!("../tests/fixtures/blockscout-native.json");
        let transaction: NativeTransaction = serde_json::from_str(fixture).unwrap();
        let transfer =
            normalize_native_transaction(&transaction, TRACKED, &EvmChain::new(1, "ETH", None))
                .unwrap();

        assert_eq!(transfer.direction, "outbound");
        assert_eq!(transfer.spent_amount, "1000000000000000000");
        assert_eq!(transfer.received_amount, "0");
        assert_eq!(transfer.asset, "ETH");
        assert_eq!(transfer.event_index, -1);
    }

    #[test]
    fn parses_successful_internal_native_transfer_and_skips_failed_calls() {
        let fixture = include_str!("../tests/fixtures/blockscout-internal.json");
        let transaction: InternalTransaction = serde_json::from_str(fixture).unwrap();
        let transfer = normalize_internal_transaction(
            &transaction,
            TRACKED,
            &EvmChain::new(8453, "ETH", None),
        )
        .unwrap();
        assert_eq!(transfer.direction, "outbound");
        assert_eq!(transfer.spent_amount, "42000000000000000");
        assert_eq!(transfer.transfer_kind, "evm_internal");
        assert_eq!(transfer.event_index, 12);
        assert_eq!(transfer.asset, "ETH");

        let failed = serde_json::from_str::<InternalTransaction>(
            &fixture.replace("\"success\": true", "\"success\": false"),
        )
        .unwrap();
        assert!(
            normalize_internal_transaction(&failed, TRACKED, &EvmChain::new(8453, "ETH", None))
                .is_none()
        );
    }

    #[test]
    fn parses_erc20_raw_amount_decimals_direction_and_log_identity() {
        let fixture = include_str!("../tests/fixtures/blockscout-erc20.json");
        let transfers: Vec<TokenTransfer> = serde_json::from_str(fixture).unwrap();
        let first = normalize_token_transfer(&transfers[0], TRACKED).unwrap();
        let second = normalize_token_transfer(&transfers[1], TRACKED).unwrap();

        assert_eq!(first.direction, "outbound");
        assert_eq!(first.spent_amount, "1500000");
        assert_eq!(first.token_decimals, Some(6));
        assert_eq!(first.asset, "USDC");
        assert_eq!(
            first.token_contract_address.as_deref(),
            Some("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        );
        assert_eq!(first.transaction_hash, second.transaction_hash);
        assert_ne!(first.event_index, second.event_index);
        assert_eq!(second.direction, "inbound");
        assert_eq!(second.received_amount, "2500000000000000000");
        assert_eq!(second.token_decimals, Some(18));
    }

    #[tokio::test]
    async fn follows_blockscout_next_page_params_as_query_parameters() {
        let (provider, state, server) = fixture_provider(vec![
            (
                StatusCode::OK,
                include_str!("../tests/fixtures/blockscout-native-page-1.json"),
            ),
            (
                StatusCode::OK,
                include_str!("../tests/fixtures/blockscout-native-page-2.json"),
            ),
        ])
        .await;

        let pages = provider
            .fetch_pages::<NativeTransaction>(
                &EvmChain::new(1, "ETH", None),
                TRACKED,
                "transactions",
                &[],
            )
            .await
            .unwrap();
        let requests = state.requests.lock().unwrap();
        server.abort();

        assert_eq!(pages.items.len(), 2);
        assert_eq!(pages.pages, 2);
        assert!(!pages.may_have_more);
        assert_eq!(
            requests[0].get("items_count").map(String::as_str),
            Some("50")
        );
        assert_eq!(
            requests[1].get("block_number").map(String::as_str),
            Some("199")
        );
        assert_eq!(requests[1].get("index").map(String::as_str), Some("7"));
    }

    #[tokio::test]
    async fn fetches_history_from_a_direct_instance_without_a_chain_id_path_prefix() {
        let (mut provider, state, server) = fixture_provider(vec![(
            StatusCode::OK,
            include_str!("../tests/fixtures/blockscout-native-page-2.json"),
        )])
        .await;
        provider.prefer_configured_base = false;
        let chain = EvmChain::new(10, "ETH", Some(provider.base_url.clone()));

        let pages = provider
            .fetch_pages::<NativeTransaction>(&chain, TRACKED, "transactions", &[])
            .await
            .unwrap();
        let requests = state.requests.lock().unwrap();
        server.abort();

        assert_eq!(pages.items.len(), 1);
        assert_eq!(requests.len(), 1);
    }

    #[tokio::test]
    async fn marks_history_incomplete_when_the_page_cap_is_reached() {
        let (provider, state, server) = fixture_provider(vec![(
            StatusCode::OK,
            include_str!("../tests/fixtures/blockscout-native-page-1.json"),
        )])
        .await;

        let pages = provider
            .fetch_pages_bounded::<NativeTransaction>(
                &EvmChain::new(1, "ETH", None),
                TRACKED,
                "transactions",
                &[],
                1,
            )
            .await
            .unwrap();
        let requests = state.requests.lock().unwrap();
        server.abort();

        assert_eq!(pages.pages, 1);
        assert!(pages.may_have_more);
        assert_eq!(requests.len(), 1);
    }

    #[tokio::test]
    async fn maps_provider_failures_to_a_generic_error_without_returning_body() {
        let (provider, _state, server) = fixture_provider(vec![(
            StatusCode::FORBIDDEN,
            include_str!("../tests/fixtures/blockscout-provider-error.json"),
        )])
        .await;

        let result = provider
            .fetch_pages::<NativeTransaction>(
                &EvmChain::new(1, "ETH", None),
                TRACKED,
                "transactions",
                &[],
            )
            .await;
        server.abort();

        assert!(matches!(result, Err(ApiError::ProviderUnavailable)));
    }

    #[tokio::test]
    async fn rejects_html_challenges_with_success_status() {
        let (provider, _state, server) =
            fixture_provider(vec![(StatusCode::OK, "<html>challenge</html>")]).await;

        let result = provider
            .fetch_pages::<NativeTransaction>(
                &EvmChain::new(1, "ETH", None),
                TRACKED,
                "transactions",
                &[],
            )
            .await;
        server.abort();

        assert!(matches!(result, Err(ApiError::ProviderUnavailable)));
    }

    #[tokio::test]
    async fn does_not_follow_provider_redirects() {
        let (provider, state, server) = fixture_provider(vec![
            (StatusCode::TEMPORARY_REDIRECT, "{}"),
            (
                StatusCode::OK,
                include_str!("../tests/fixtures/blockscout-native-page-2.json"),
            ),
        ])
        .await;
        let result = provider
            .fetch_pages::<NativeTransaction>(
                &EvmChain::new(1, "ETH", None),
                TRACKED,
                "transactions",
                &[],
            )
            .await;
        let request_count = state.requests.lock().unwrap().len();
        server.abort();

        assert!(matches!(result, Err(ApiError::ProviderUnavailable)));
        assert_eq!(request_count, 1);
    }

    #[tokio::test]
    async fn sends_optional_api_key_as_bearer_header() {
        let (provider, state, server) = fixture_provider(vec![(
            StatusCode::OK,
            include_str!("../tests/fixtures/blockscout-native-page-2.json"),
        )])
        .await;
        let provider =
            BlockscoutClient::new(provider.base_url.as_str(), Some("test-key"), Client::new())
                .unwrap();

        provider
            .fetch_pages::<NativeTransaction>(
                &EvmChain::new(1, "ETH", None),
                TRACKED,
                "transactions",
                &[],
            )
            .await
            .unwrap();
        let saw_key = state.saw_bearer.lock().unwrap().to_owned();
        server.abort();

        assert!(saw_key);
    }

    struct FixtureState {
        pages: Mutex<VecDeque<(StatusCode, &'static str)>>,
        requests: Mutex<Vec<BTreeMap<String, String>>>,
        saw_bearer: Mutex<bool>,
    }

    async fn fixture_handler(
        State(state): State<Arc<FixtureState>>,
        uri: Uri,
        headers: HeaderMap,
    ) -> Response<Body> {
        let query = uri.query().unwrap_or_default();
        let parsed_url = reqwest::Url::parse(&format!("http://localhost/?{query}")).unwrap();
        let values = parsed_url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        state.requests.lock().unwrap().push(values);
        if headers
            .get(axum::http::header::AUTHORIZATION)
            .is_some_and(|value| value == "Bearer test-key")
        {
            *state.saw_bearer.lock().unwrap() = true;
        }
        let (status, body) = state
            .pages
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or((StatusCode::INTERNAL_SERVER_ERROR, "{}"));
        let mut response = Response::builder().status(status);
        if status.is_redirection() {
            response = response.header(
                axum::http::header::LOCATION,
                "/1/api/v2/addresses/0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/transactions",
            );
        }
        response.body(Body::from(body)).unwrap()
    }

    async fn fixture_provider(
        pages: Vec<(StatusCode, &'static str)>,
    ) -> (BlockscoutClient, Arc<FixtureState>, JoinHandle<()>) {
        let state = Arc::new(FixtureState {
            pages: Mutex::new(pages.into()),
            requests: Mutex::new(Vec::new()),
            saw_bearer: Mutex::new(false),
        });
        let app = Router::new()
            .route(
                "/{chain_id}/api/v2/addresses/{address}/{endpoint}",
                get(fixture_handler),
            )
            .route(
                "/api/v2/addresses/{address}/{endpoint}",
                get(fixture_handler),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let provider = BlockscoutClient::new(&format!("http://{address}"), None, client).unwrap();
        (provider, state, server)
    }
}
