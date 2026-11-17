use std::collections::{HashMap, HashSet};
use std::{env, net::IpAddr};

use axum::http::HeaderValue;
use chrono::{DateTime, TimeZone, Utc};
use reqwest::{Client, Url};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::{ApiState, error::ApiError, transfers::SyncResult};

const DEFAULT_BASE_URL: &str = "https://api.trongrid.io";
const PAGE_SIZE: &str = "200";
const MAX_PAGES_PER_STREAM: usize = 3;
const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024;
const API_KEY_HEADER: &str = "TRON-PRO-API-KEY";

#[derive(Clone)]
pub struct TronGridClient {
    client: Client,
    base_url: Url,
    api_key: Option<HeaderValue>,
}

#[derive(Debug, Error)]
pub enum TronGridConfigError {
    #[error("TRONGRID_BASE_URL is invalid")]
    InvalidBaseUrl,
    #[error("TRONGRID_BASE_URL must use HTTPS outside loopback development")]
    InsecureBaseUrl,
    #[error("TRONGRID_API_KEY contains invalid header characters")]
    InvalidApiKey,
}

#[derive(Debug, Deserialize)]
struct TronGridPage<T> {
    success: Option<bool>,
    data: Vec<T>,
    meta: Option<TronGridMeta>,
}

#[derive(Debug, Deserialize)]
struct TronGridMeta {
    fingerprint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TronTransaction {
    #[serde(rename = "txID")]
    transaction_id: String,
    #[serde(rename = "blockNumber")]
    block_number: Option<i64>,
    #[serde(rename = "block_timestamp")]
    block_timestamp: Option<i64>,
    raw_data: Option<TronRawData>,
    #[serde(default)]
    ret: Vec<TronTransactionResult>,
}

#[derive(Debug, Deserialize)]
struct TronRawData {
    #[serde(default)]
    contract: Vec<TronContract>,
}

#[derive(Debug, Deserialize)]
struct TronContract {
    #[serde(rename = "type")]
    contract_type: String,
    parameter: Option<TronContractParameter>,
}

#[derive(Debug, Deserialize)]
struct TronContractParameter {
    value: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct TronTransactionResult {
    #[serde(rename = "contractRet")]
    contract_result: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct Trc20Transfer {
    transaction_id: String,
    #[serde(rename = "block_timestamp")]
    block_timestamp: Option<i64>,
    #[serde(rename = "block_number")]
    block_number: Option<i64>,
    token_info: Option<Trc20TokenInfo>,
    #[serde(rename = "type")]
    event_type: Option<String>,
    #[serde(default, alias = "eventIndex", alias = "log_index")]
    event_index: Option<i64>,
    from: String,
    to: String,
    value: Value,
}

#[derive(Clone, Debug, Deserialize)]
struct Trc20TokenInfo {
    address: String,
    symbol: Option<String>,
    decimals: Option<u8>,
}

trait TransactionId {
    fn transaction_id(&self) -> &str;
}

impl TransactionId for TronTransaction {
    fn transaction_id(&self) -> &str {
        &self.transaction_id
    }
}

impl TransactionId for Trc20Transfer {
    fn transaction_id(&self) -> &str {
        &self.transaction_id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stream {
    Native,
    Trc20,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct SyncCursor {
    stream: String,
    head_transaction_id: Option<String>,
    pending_head_transaction_id: Option<String>,
    pending_fingerprint: Option<String>,
    backfill_fingerprint: Option<String>,
    backfill_exhausted: bool,
}

#[derive(Clone, Debug)]
struct NextCursor {
    head_transaction_id: Option<String>,
    pending_head_transaction_id: Option<String>,
    pending_fingerprint: Option<String>,
    backfill_fingerprint: Option<String>,
    backfill_exhausted: bool,
}

#[derive(Debug)]
struct NormalizedTransfer {
    transaction_hash: String,
    event_index: i64,
    transfer_kind: &'static str,
    direction: &'static str,
    received_amount: String,
    spent_amount: String,
    asset: String,
    token_contract_address: Option<String>,
    token_decimals: Option<i16>,
    from_address: String,
    to_address: String,
    block_height: Option<i64>,
    block_time: Option<DateTime<Utc>>,
}

struct Trc20Candidate {
    record: Trc20Transfer,
    transaction_hash: String,
    contract: String,
    from_address: String,
    to_address: String,
    amount: String,
    direction: &'static str,
    asset: String,
    token_decimals: Option<i16>,
}

#[derive(Debug)]
struct RawStreamBatch<T> {
    records: Vec<T>,
    next_cursor: NextCursor,
    fetched: usize,
    pages: usize,
    may_have_more: bool,
}

impl TronGridClient {
    pub fn from_env(client: Client) -> Result<Self, TronGridConfigError> {
        let base_url = env::var("TRONGRID_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());
        let api_key = env::var("TRONGRID_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty());
        Self::new(&base_url, api_key.as_deref(), client)
    }

    pub fn new(
        base_url: &str,
        api_key: Option<&str>,
        client: Client,
    ) -> Result<Self, TronGridConfigError> {
        let base_url =
            Url::parse(base_url.trim()).map_err(|_| TronGridConfigError::InvalidBaseUrl)?;
        let host = base_url
            .host_str()
            .ok_or(TronGridConfigError::InvalidBaseUrl)?;
        if !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
            || !matches!(base_url.path(), "" | "/")
        {
            return Err(TronGridConfigError::InvalidBaseUrl);
        }
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        if base_url.scheme() != "https" && !(base_url.scheme() == "http" && loopback) {
            return Err(TronGridConfigError::InsecureBaseUrl);
        }
        let api_key = api_key
            .map(|key| HeaderValue::try_from(key).map_err(|_| TronGridConfigError::InvalidApiKey))
            .transpose()?;
        Ok(Self {
            client,
            base_url,
            api_key,
        })
    }

    pub fn is_configured(&self) -> bool {
        self.api_key.is_some()
    }

    fn source_host(&self) -> &str {
        self.base_url.host_str().unwrap_or("unknown")
    }

    fn build_request(
        &self,
        address: &str,
        stream: Stream,
        fingerprint: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let endpoint = match stream {
            Stream::Native => "transactions",
            Stream::Trc20 => "transactions/trc20",
        };
        let url = self
            .base_url
            .join(&format!("v1/accounts/{address}/{endpoint}"))
            .expect("validated base URL should accept a static endpoint");
        let mut request = self.client.get(url).query(&[
            ("only_confirmed", "true"),
            ("limit", PAGE_SIZE),
            ("order_by", "block_timestamp,desc"),
        ]);
        if let Some(fingerprint) = fingerprint {
            request = request.query(&[("fingerprint", fingerprint)]);
        }
        if let Some(api_key) = &self.api_key {
            request = request.header(API_KEY_HEADER, api_key.clone());
        }
        request
    }

    async fn fetch_page<T: DeserializeOwned>(
        &self,
        address: &str,
        stream: Stream,
        fingerprint: Option<&str>,
    ) -> Result<TronGridPage<T>, ApiError> {
        let mut response = self
            .build_request(address, stream, fingerprint)
            .send()
            .await
            .map_err(|_| ApiError::ProviderUnavailable)?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > MAX_PAGE_BYTES as u64)
        {
            // 403/429 и другие ошибки провайдера повторяются только при следующей синхронизации.
            return Err(ApiError::ProviderUnavailable);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ApiError::ProviderUnavailable)?
        {
            if body.len() + chunk.len() > MAX_PAGE_BYTES {
                return Err(ApiError::ProviderUnavailable);
            }
            body.extend_from_slice(&chunk);
        }
        let page: TronGridPage<T> =
            serde_json::from_slice(&body).map_err(|_| ApiError::ProviderUnavailable)?;
        if page.success != Some(true) {
            return Err(ApiError::ProviderUnavailable);
        }
        Ok(page)
    }
}

pub(crate) async fn sync_tracked_address(
    state: &ApiState,
    address_id: Uuid,
    address: &str,
) -> Result<SyncResult, ApiError> {
    if !state.tron_grid.is_configured() {
        return Err(ApiError::ProviderUnavailable);
    }
    let cursors = sqlx::query_as::<_, SyncCursor>(
        "SELECT stream, head_transaction_id, pending_head_transaction_id, pending_fingerprint, \
         backfill_fingerprint, backfill_exhausted FROM tron_sync_cursors \
         WHERE tracked_address_id = $1",
    )
    .bind(address_id)
    .fetch_all(&state.database)
    .await?;
    let native_cursor = cursors.iter().find(|cursor| cursor.stream == "native");
    let trc20_cursor = cursors.iter().find(|cursor| cursor.stream == "trc20");
    let (native_raw, trc20_raw) = tokio::try_join!(
        sync_stream::<TronTransaction>(&state.tron_grid, address, Stream::Native, native_cursor),
        sync_stream::<Trc20Transfer>(&state.tron_grid, address, Stream::Trc20, trc20_cursor),
    )?;
    let native_transfers = normalize_native_batch(native_raw.records, address);
    let trc20_transfers = normalize_trc20_batch(trc20_raw.records, address);

    let observed_at = Utc::now();
    let mut transaction = state.database.begin().await?;
    let mut saved = 0;
    for transfer in native_transfers.iter().chain(&trc20_transfers) {
        saved += sqlx::query(
            "INSERT INTO observed_transfers \
             (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, \
              received_amount, spent_amount, asset, token_contract_address, token_decimals, from_address, \
              to_address, block_height, block_time, confirmed, provider, source_host, observed_at) \
             VALUES ($1, 'tron', $2, $3, $4, $5, $6::numeric, $7::numeric, $8, $9, $10, $11, $12, $13, $14, TRUE, 'TronGrid', $15, $16) \
             ON CONFLICT (tracked_address_id, transaction_hash, transfer_kind, event_index) \
             DO UPDATE SET direction = EXCLUDED.direction, received_amount = EXCLUDED.received_amount, \
              spent_amount = EXCLUDED.spent_amount, asset = EXCLUDED.asset, \
              token_contract_address = EXCLUDED.token_contract_address, token_decimals = EXCLUDED.token_decimals, \
              from_address = EXCLUDED.from_address, to_address = EXCLUDED.to_address, \
              block_height = EXCLUDED.block_height, block_time = EXCLUDED.block_time, \
              confirmed = EXCLUDED.confirmed, provider = EXCLUDED.provider, source_host = EXCLUDED.source_host, \
              observed_at = EXCLUDED.observed_at",
        )
        .bind(address_id)
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
        .bind(state.tron_grid.source_host())
        .bind(observed_at)
        .execute(&mut *transaction)
        .await?
        .rows_affected() as usize;
    }
    for (stream, cursor) in [
        ("native", &native_raw.next_cursor),
        ("trc20", &trc20_raw.next_cursor),
    ] {
        save_cursor(&mut transaction, address_id, stream, cursor, observed_at).await?;
    }
    transaction.commit().await?;

    let fetched = native_raw.fetched + trc20_raw.fetched;
    let pages = native_raw.pages + trc20_raw.pages;
    Ok(SyncResult {
        provider: format!("TronGrid ({})", state.tron_grid.source_host()),
        fetched,
        saved,
        pages,
        may_have_more_history: native_raw.may_have_more || trc20_raw.may_have_more,
        observed_at,
    })
}

async fn sync_stream<T: DeserializeOwned + TransactionId>(
    client: &TronGridClient,
    address: &str,
    stream: Stream,
    cursor: Option<&SyncCursor>,
) -> Result<RawStreamBatch<T>, ApiError> {
    let mut next_cursor = cursor.map_or(
        NextCursor {
            head_transaction_id: None,
            pending_head_transaction_id: None,
            pending_fingerprint: None,
            backfill_fingerprint: None,
            backfill_exhausted: false,
        },
        |cursor| NextCursor {
            head_transaction_id: cursor.head_transaction_id.clone(),
            pending_head_transaction_id: cursor.pending_head_transaction_id.clone(),
            pending_fingerprint: cursor.pending_fingerprint.clone(),
            backfill_fingerprint: cursor.backfill_fingerprint.clone(),
            backfill_exhausted: cursor.backfill_exhausted,
        },
    );
    let mut records: Vec<T> = Vec::new();
    let mut pages = 0;

    if next_cursor.head_transaction_id.is_none() {
        let mut fingerprint = None;
        while pages < MAX_PAGES_PER_STREAM {
            let page = client
                .fetch_page(address, stream, fingerprint.as_deref())
                .await?;
            pages += 1;
            validate_page_transaction_ids(&page.data)?;
            if pages == 1 {
                next_cursor.head_transaction_id = page
                    .data
                    .first()
                    .map(|record: &T| record.transaction_id().to_ascii_lowercase());
            }
            records.extend(page.data);
            fingerprint = validated_next_fingerprint(page.meta, fingerprint.as_deref())?;
            if fingerprint.is_none() {
                break;
            }
        }
        next_cursor.backfill_fingerprint = fingerprint.clone();
        next_cursor.backfill_exhausted = fingerprint.is_none();
    } else {
        let checkpoint = next_cursor
            .head_transaction_id
            .as_deref()
            .expect("head transaction id was checked above");
        let mut pending_head = next_cursor.pending_head_transaction_id.clone();
        let mut fingerprint = next_cursor.pending_fingerprint.clone();
        let mut reached_checkpoint = false;
        while pages < MAX_PAGES_PER_STREAM {
            let page = client
                .fetch_page(address, stream, fingerprint.as_deref())
                .await?;
            pages += 1;
            validate_page_transaction_ids(&page.data)?;
            if pending_head.is_none() {
                pending_head = page
                    .data
                    .first()
                    .map(|record: &T| record.transaction_id().to_ascii_lowercase());
            }
            let page_fingerprint = validated_next_fingerprint(page.meta, fingerprint.as_deref())?;
            let (new_records, found) = take_through_checkpoint(page.data, checkpoint);
            records.extend(new_records);
            if found {
                next_cursor.head_transaction_id = pending_head.clone();
                next_cursor.pending_head_transaction_id = None;
                next_cursor.pending_fingerprint = None;
                reached_checkpoint = true;
                break;
            }
            if page_fingerprint.is_none() {
                // Старый checkpoint мог быть удалён из индекса провайдера.
                next_cursor.head_transaction_id = pending_head.clone();
                next_cursor.pending_head_transaction_id = None;
                next_cursor.pending_fingerprint = None;
                reached_checkpoint = true;
                break;
            }
            next_cursor.pending_head_transaction_id = pending_head.clone();
            next_cursor.pending_fingerprint = page_fingerprint.clone();
            fingerprint = page_fingerprint;
        }

        if reached_checkpoint && pages < MAX_PAGES_PER_STREAM && !next_cursor.backfill_exhausted {
            let mut backfill_fingerprint = next_cursor.backfill_fingerprint.clone();
            while pages < MAX_PAGES_PER_STREAM {
                let Some(current_fingerprint) = backfill_fingerprint.as_deref() else {
                    next_cursor.backfill_exhausted = true;
                    break;
                };
                let page = client
                    .fetch_page(address, stream, Some(current_fingerprint))
                    .await?;
                pages += 1;
                validate_page_transaction_ids(&page.data)?;
                records.extend(page.data);
                backfill_fingerprint =
                    validated_next_fingerprint(page.meta, Some(current_fingerprint))?;
                if backfill_fingerprint.is_none() {
                    next_cursor.backfill_exhausted = true;
                    break;
                }
            }
            next_cursor.backfill_fingerprint = backfill_fingerprint;
        }
    }

    let may_have_more = next_cursor.pending_fingerprint.is_some()
        || next_cursor.backfill_fingerprint.is_some()
        || !next_cursor.backfill_exhausted;
    let fetched = records.len();
    Ok(RawStreamBatch {
        records,
        next_cursor,
        fetched,
        pages,
        may_have_more,
    })
}

fn validate_page_transaction_ids<T: TransactionId>(records: &[T]) -> Result<(), ApiError> {
    if records
        .iter()
        .any(|record| !valid_transaction_id(record.transaction_id()))
    {
        return Err(ApiError::ProviderUnavailable);
    }
    Ok(())
}

fn validated_next_fingerprint(
    meta: Option<TronGridMeta>,
    current: Option<&str>,
) -> Result<Option<String>, ApiError> {
    let fingerprint = meta
        .and_then(|meta| meta.fingerprint)
        .filter(|fingerprint| !fingerprint.trim().is_empty());
    if fingerprint.as_deref() == current {
        return Err(ApiError::ProviderUnavailable);
    }
    Ok(fingerprint)
}

fn take_through_checkpoint<T: TransactionId>(records: Vec<T>, checkpoint: &str) -> (Vec<T>, bool) {
    let mut reached_checkpoint = false;
    let mut selected = Vec::new();
    for record in records {
        let is_checkpoint = record.transaction_id().eq_ignore_ascii_case(checkpoint);
        if reached_checkpoint && !is_checkpoint {
            break;
        }
        reached_checkpoint |= is_checkpoint;
        selected.push(record);
    }
    (selected, reached_checkpoint)
}

fn normalize_native_batch(
    records: Vec<TronTransaction>,
    tracked_address: &str,
) -> Vec<NormalizedTransfer> {
    let mut transfers = Vec::new();
    for transaction in records {
        if !valid_transaction_id(&transaction.transaction_id) {
            continue;
        }
        let Some(raw_data) = transaction.raw_data else {
            continue;
        };
        for (index, contract) in raw_data.contract.iter().enumerate() {
            if contract.contract_type != "TransferContract"
                || transaction
                    .ret
                    .get(index)
                    .and_then(|result| result.contract_result.as_deref())
                    != Some("SUCCESS")
            {
                continue;
            }
            let Some(value) = contract
                .parameter
                .as_ref()
                .and_then(|parameter| parameter.value.as_ref())
            else {
                continue;
            };
            let Some(from_address) = value
                .get("owner_address")
                .and_then(Value::as_str)
                .and_then(|address| crate::addresses::normalize_tron_address(address).ok())
            else {
                continue;
            };
            let Some(to_address) = value
                .get("to_address")
                .and_then(Value::as_str)
                .and_then(|address| crate::addresses::normalize_tron_address(address).ok())
            else {
                continue;
            };
            let Some(amount) = value.get("amount").and_then(parse_amount) else {
                continue;
            };
            if amount == "0" {
                continue;
            }
            let direction = if from_address == tracked_address && to_address == tracked_address {
                "both"
            } else if from_address == tracked_address {
                "outbound"
            } else if to_address == tracked_address {
                "inbound"
            } else {
                continue;
            };
            let Ok(event_index) = i64::try_from(index) else {
                continue;
            };
            transfers.push(NormalizedTransfer {
                transaction_hash: transaction.transaction_id.to_ascii_lowercase(),
                event_index,
                transfer_kind: "tron_native",
                direction,
                received_amount: if matches!(direction, "inbound" | "both") {
                    amount.clone()
                } else {
                    "0".into()
                },
                spent_amount: if matches!(direction, "outbound" | "both") {
                    amount
                } else {
                    "0".into()
                },
                asset: "TRX".into(),
                token_contract_address: None,
                token_decimals: Some(6),
                from_address,
                to_address,
                block_height: transaction.block_number.filter(|height| *height >= 0),
                block_time: transaction
                    .block_timestamp
                    .and_then(|timestamp| Utc.timestamp_millis_opt(timestamp).single()),
            });
        }
    }
    transfers
}

fn normalize_trc20_batch(
    records: Vec<Trc20Transfer>,
    tracked_address: &str,
) -> Vec<NormalizedTransfer> {
    let mut candidates = Vec::new();
    for record in records {
        if !valid_transaction_id(&record.transaction_id)
            || record.event_type.as_deref() != Some("Transfer")
        {
            continue;
        }
        let Some(token) = record.token_info.clone() else {
            continue;
        };
        let Some(contract) = crate::addresses::normalize_tron_address(&token.address).ok() else {
            continue;
        };
        let Some(from_address) = crate::addresses::normalize_tron_address(&record.from).ok() else {
            continue;
        };
        let Some(to_address) = crate::addresses::normalize_tron_address(&record.to).ok() else {
            continue;
        };
        let Some(amount) = parse_amount(&record.value) else {
            continue;
        };
        if amount == "0" {
            continue;
        }
        let direction = if from_address == tracked_address && to_address == tracked_address {
            "both"
        } else if from_address == tracked_address {
            "outbound"
        } else if to_address == tracked_address {
            "inbound"
        } else {
            continue;
        };
        let asset = token
            .symbol
            .filter(|symbol| {
                !symbol.is_empty()
                    && symbol.len() <= 16
                    && symbol
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
            })
            .unwrap_or_else(|| "TRC-20".into());
        candidates.push(Trc20Candidate {
            transaction_hash: record.transaction_id.to_ascii_lowercase(),
            contract,
            from_address,
            to_address,
            amount,
            direction,
            asset,
            token_decimals: token.decimals.map(i16::from),
            record,
        });
    }
    candidates.sort_by(|left, right| {
        (
            &left.transaction_hash,
            &left.contract,
            &left.from_address,
            &left.to_address,
            &left.amount,
            left.record.event_index,
        )
            .cmp(&(
                &right.transaction_hash,
                &right.contract,
                &right.from_address,
                &right.to_address,
                &right.amount,
                right.record.event_index,
            ))
    });

    let mut transfers = Vec::with_capacity(candidates.len());
    let mut occurrences = HashMap::<String, usize>::new();
    let mut used_indexes = HashSet::<(String, i64)>::new();
    for candidate in candidates {
        let semantic_key = format!(
            "{}|{}|{}|{}|{}",
            candidate.transaction_hash,
            candidate.contract,
            candidate.from_address,
            candidate.to_address,
            candidate.amount
        );
        let occurrence = occurrences.entry(semantic_key.clone()).or_default();
        let mut event_index = candidate
            .record
            .event_index
            .filter(|index| *index >= 0)
            .unwrap_or_else(|| stable_event_index(&format!("{semantic_key}|{occurrence}")));
        *occurrence += 1;
        let mut collision = 0_u32;
        while !used_indexes.insert((candidate.transaction_hash.clone(), event_index)) {
            event_index = stable_event_index(&format!(
                "{semantic_key}|{occurrence}|collision:{collision}"
            ));
            collision = collision.saturating_add(1);
        }
        transfers.push(NormalizedTransfer {
            transaction_hash: candidate.transaction_hash,
            event_index,
            transfer_kind: "trc20",
            direction: candidate.direction,
            received_amount: if matches!(candidate.direction, "inbound" | "both") {
                candidate.amount.clone()
            } else {
                "0".into()
            },
            spent_amount: if matches!(candidate.direction, "outbound" | "both") {
                candidate.amount
            } else {
                "0".into()
            },
            asset: candidate.asset,
            token_contract_address: Some(candidate.contract),
            token_decimals: candidate.token_decimals,
            from_address: candidate.from_address,
            to_address: candidate.to_address,
            block_height: candidate.record.block_number.filter(|height| *height >= 0),
            block_time: candidate
                .record
                .block_timestamp
                .and_then(|timestamp| Utc.timestamp_millis_opt(timestamp).single()),
        });
    }
    transfers
}

fn parse_amount(value: &Value) -> Option<String> {
    let raw = value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|amount| amount.to_string()))?;
    if raw.is_empty() || raw.len() > 78 || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let canonical = raw.trim_start_matches('0');
    Some(if canonical.is_empty() { "0" } else { canonical }.to_owned())
}

fn valid_transaction_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn stable_event_index(identity: &str) -> i64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in identity.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    (hash & i64::MAX as u64).max(1) as i64
}

async fn save_cursor(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    address_id: Uuid,
    stream: &str,
    cursor: &NextCursor,
    updated_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO tron_sync_cursors \
         (tracked_address_id, stream, head_transaction_id, pending_head_transaction_id, \
          pending_fingerprint, backfill_fingerprint, backfill_exhausted, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (tracked_address_id, stream) DO UPDATE SET \
          head_transaction_id = EXCLUDED.head_transaction_id, \
          pending_head_transaction_id = EXCLUDED.pending_head_transaction_id, \
          pending_fingerprint = EXCLUDED.pending_fingerprint, \
          backfill_fingerprint = EXCLUDED.backfill_fingerprint, \
          backfill_exhausted = EXCLUDED.backfill_exhausted, updated_at = EXCLUDED.updated_at",
    )
    .bind(address_id)
    .bind(stream)
    .bind(&cursor.head_transaction_id)
    .bind(&cursor.pending_head_transaction_id)
    .bind(&cursor.pending_fingerprint)
    .bind(&cursor.backfill_fingerprint)
    .bind(cursor.backfill_exhausted)
    .bind(updated_at)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use axum::{
        Json, Router,
        extract::{Query, State},
        http::StatusCode,
        routing::get,
    };
    use serde_json::{Value, json};

    use super::{
        Stream, SyncCursor, Trc20TokenInfo, Trc20Transfer, TronGridClient, TronTransaction,
        normalize_native_batch, normalize_trc20_batch, parse_amount, sync_stream,
        take_through_checkpoint, validated_next_fingerprint,
    };

    const TRACKED: &str = "TUoHaVjx7n5xz8LwPRDckgFrDWhMhuSuJM";
    const TRACKED_HEX: &str = "41ce8a0cf0c16d48bcf22825f6053248df653c89ca";
    const RECIPIENT: &str = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t";
    const RECIPIENT_HEX: &str = "41a614f803b6fd780986a42c78ec9c7f77e6ded13c";
    const MINT: &str = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t";

    fn trc20_record(amount: &str, to: &str) -> Trc20Transfer {
        Trc20Transfer {
            transaction_id: "a1".repeat(32),
            block_timestamp: Some(1_700_000_000_000),
            block_number: Some(10),
            token_info: Some(Trc20TokenInfo {
                address: MINT.into(),
                symbol: Some("USDT".into()),
                decimals: Some(6),
            }),
            event_type: Some("Transfer".into()),
            event_index: None,
            from: TRACKED_HEX.into(),
            to: to.into(),
            value: Value::String(amount.into()),
        }
    }

    #[test]
    fn keeps_same_transaction_trc20_rows_distinct_and_stable_when_reordered() {
        let records = vec![
            trc20_record("100", RECIPIENT),
            trc20_record("200", TRACKED),
            trc20_record("100", RECIPIENT),
            Trc20Transfer {
                event_type: Some("Approval".into()),
                ..trc20_record("999", RECIPIENT)
            },
        ];
        let mut reversed = records.clone();
        reversed.reverse();

        let first = normalize_trc20_batch(records, TRACKED);
        let second = normalize_trc20_batch(reversed, TRACKED);
        assert_eq!(first.len(), 3);
        assert_eq!(second.len(), 3);
        let mut first_events = first
            .iter()
            .map(|transfer| {
                (
                    transfer.event_index,
                    transfer.received_amount.clone(),
                    transfer.spent_amount.clone(),
                )
            })
            .collect::<Vec<_>>();
        let mut second_events = second
            .iter()
            .map(|transfer| {
                (
                    transfer.event_index,
                    transfer.received_amount.clone(),
                    transfer.spent_amount.clone(),
                )
            })
            .collect::<Vec<_>>();
        first_events.sort();
        second_events.sort();
        assert_eq!(first_events, second_events);
        assert_eq!(
            first
                .iter()
                .map(|transfer| transfer.event_index)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            3
        );
        assert!(
            first
                .iter()
                .all(|transfer| transfer.from_address == TRACKED)
        );
        assert_eq!(first[0].token_contract_address.as_deref(), Some(MINT));
    }

    #[test]
    fn accepts_provider_event_index_and_ignores_invalid_event_rows() {
        let mut record = trc20_record("100", RECIPIENT);
        record.event_index = Some(77);
        let invalid = Trc20Transfer {
            transaction_id: "not-a-hash".into(),
            ..trc20_record("100", RECIPIENT)
        };
        let transfers = normalize_trc20_batch(vec![record, invalid], TRACKED);
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].event_index, 77);
    }

    #[test]
    fn native_history_only_keeps_successful_transfer_contracts() {
        let transaction: TronTransaction = serde_json::from_value(json!({
            "txID": "ab".repeat(32),
            "blockNumber": 10,
            "block_timestamp": 1700000000000_i64,
            "raw_data": { "contract": [
                { "type": "TransferContract", "parameter": { "value": {
                    "owner_address": TRACKED_HEX,
                    "to_address": RECIPIENT_HEX,
                    "amount": 2500000
                } } },
                { "type": "TriggerSmartContract", "parameter": { "value": {
                    "owner_address": TRACKED_HEX
                } } },
                { "type": "TransferContract", "parameter": { "value": {
                    "owner_address": TRACKED_HEX,
                    "to_address": RECIPIENT_HEX,
                    "amount": 9000000
                } } }
            ] },
            "ret": [
                { "contractRet": "SUCCESS" },
                { "contractRet": "SUCCESS" },
                { "contractRet": "REVERT" }
            ]
        }))
        .unwrap();

        let transfers = normalize_native_batch(vec![transaction], TRACKED);
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].direction, "outbound");
        assert_eq!(transfers[0].spent_amount, "2500000");
        assert_eq!(transfers[0].from_address, TRACKED);
        assert_eq!(transfers[0].to_address, RECIPIENT);
    }

    #[test]
    fn amount_parser_bounds_values_and_canonicalizes_zeroes() {
        assert_eq!(parse_amount(&json!("00042")).as_deref(), Some("42"));
        assert_eq!(parse_amount(&json!("000")).as_deref(), Some("0"));
        assert_eq!(parse_amount(&json!("1".repeat(78))).unwrap().len(), 78);
        assert!(parse_amount(&json!("1".repeat(79))).is_none());
        assert!(parse_amount(&json!("-1")).is_none());
    }

    #[test]
    fn api_key_is_sent_only_as_a_header_and_endpoint_config_is_validated() {
        let key = "secret-for-test";
        let client =
            TronGridClient::new("https://api.trongrid.io", Some(key), reqwest::Client::new())
                .unwrap();
        let request = client
            .build_request(TRACKED, Stream::Trc20, Some("page-fingerprint"))
            .build()
            .unwrap();
        assert!(client.is_configured());
        assert!(!request.url().as_str().contains(key));
        assert_eq!(
            request
                .url()
                .query_pairs()
                .find(|(name, _)| name == "fingerprint")
                .unwrap()
                .1,
            "page-fingerprint"
        );
        assert_eq!(request.headers()["tron-pro-api-key"], key);
        assert!(
            TronGridClient::new("https://api.trongrid.io/path", None, reqwest::Client::new())
                .is_err()
        );
        assert!(
            TronGridClient::new("http://trongrid.example", None, reqwest::Client::new()).is_err()
        );
        assert!(TronGridClient::new("http://127.0.0.1:8081", None, reqwest::Client::new()).is_ok());
        assert!(
            TronGridClient::new(
                "https://api.trongrid.io",
                Some("bad\nkey"),
                reqwest::Client::new()
            )
            .is_err()
        );
    }

    #[derive(Clone)]
    struct MockPages(Arc<AtomicUsize>);

    async fn mock_account_page(
        State(calls): State<MockPages>,
        Query(query): Query<HashMap<String, String>>,
    ) -> Json<Value> {
        calls.0.fetch_add(1, Ordering::SeqCst);
        let page = if query.get("fingerprint").map(String::as_str) == Some("older-page") {
            json!({
                "success": true,
                "data": [
                    { "txID": "03".repeat(32) },
                    { "txID": "04".repeat(32) }
                ],
                "meta": { "fingerprint": "older-page-2" }
            })
        } else {
            json!({
                "success": true,
                "data": [
                    { "txID": "01".repeat(32) },
                    { "txID": "02".repeat(32) }
                ],
                "meta": { "fingerprint": "older-page" }
            })
        };
        Json(page)
    }

    async fn mock_rate_limited_page(State(calls): State<MockPages>) -> (StatusCode, Json<Value>) {
        calls.0.fetch_add(1, Ordering::SeqCst);
        (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error": "rate limited"})),
        )
    }

    #[tokio::test]
    async fn does_not_retry_rate_limited_provider_requests() {
        let calls = MockPages(Arc::new(AtomicUsize::new(0)));
        let app = Router::new()
            .route(
                "/v1/accounts/{address}/transactions",
                get(mock_rate_limited_page),
            )
            .with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = TronGridClient::new(&base_url, None, reqwest::Client::new()).unwrap();

        let result = client
            .fetch_page::<TronTransaction>(TRACKED, Stream::Native, None)
            .await;
        server.abort();

        assert!(result.is_err());
        assert_eq!(calls.0.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn advances_forward_cursor_over_opaque_fingerprint_pages() {
        let calls = MockPages(Arc::new(AtomicUsize::new(0)));
        let app = Router::new()
            .route(
                "/v1/accounts/{address}/transactions",
                get(mock_account_page),
            )
            .with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = TronGridClient::new(&base_url, None, reqwest::Client::new()).unwrap();
        let cursor = SyncCursor {
            stream: "native".into(),
            head_transaction_id: Some("03".repeat(32)),
            pending_head_transaction_id: None,
            pending_fingerprint: None,
            backfill_fingerprint: None,
            backfill_exhausted: true,
        };

        let batch = sync_stream::<TronTransaction>(&client, TRACKED, Stream::Native, Some(&cursor))
            .await
            .unwrap();
        server.abort();

        assert_eq!(calls.0.load(Ordering::SeqCst), 2);
        assert_eq!(batch.fetched, 3);
        assert_eq!(batch.next_cursor.head_transaction_id, Some("01".repeat(32)));
        assert!(batch.next_cursor.pending_fingerprint.is_none());
        assert_eq!(batch.records[2].transaction_id, "03".repeat(32));
    }

    #[test]
    fn cursor_helpers_stop_after_all_rows_of_checkpoint_transaction() {
        #[derive(Debug)]
        struct Row(String);
        impl super::TransactionId for Row {
            fn transaction_id(&self) -> &str {
                &self.0
            }
        }

        let (rows, found) = take_through_checkpoint(
            vec![
                Row("new".into()),
                Row("head".into()),
                Row("head".into()),
                Row("old".into()),
            ],
            "head",
        );
        assert!(found);
        assert_eq!(rows.len(), 3);
        assert!(
            validated_next_fingerprint(
                Some(super::TronGridMeta {
                    fingerprint: Some("same".into())
                }),
                Some("same")
            )
            .is_err()
        );
    }
}
