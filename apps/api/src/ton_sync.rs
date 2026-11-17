use std::{env, net::IpAddr, sync::Arc, time::Duration};

use axum::http::HeaderValue;
use chrono::{DateTime, TimeZone, Utc};
use reqwest::{Client, Url};
use serde_json::Value;
use sqlx::FromRow;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::time::Instant;
use uuid::Uuid;

use crate::{ApiState, error::ApiError, transfers::SyncResult};

const DEFAULT_BASE_URL: &str = "https://toncenter.com/api/v3/";
const PAGE_SIZE: usize = 100;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const PROVIDER: &str = "TON Center API v3";

#[derive(Clone)]
pub struct TonCenterClient {
    client: Client,
    base_url: Url,
    api_key: Option<HeaderValue>,
    unauthenticated_request_gate: Arc<Mutex<Option<Instant>>>,
}

#[derive(Debug, Error)]
pub enum TonCenterConfigError {
    #[error("TONCENTER_BASE_URL is invalid")]
    InvalidBaseUrl,
    #[error("TONCENTER_BASE_URL must use HTTPS outside loopback development")]
    InsecureBaseUrl,
    #[error("TONCENTER_API_KEY contains invalid header characters")]
    InvalidApiKey,
}

#[derive(Debug, FromRow)]
struct SyncCursor {
    native_newest_lt: Option<i64>,
    native_backfill_end_lt: Option<i64>,
    native_backfill_exhausted: bool,
    jetton_newest_lt: Option<i64>,
    jetton_backfill_end_lt: Option<i64>,
    jetton_backfill_exhausted: bool,
}

#[derive(Debug)]
struct NativeTransfer {
    hash: String,
    event_index: i64,
    direction: &'static str,
    received: String,
    spent: String,
    from: String,
    to: String,
    block_height: Option<i64>,
    block_time: Option<DateTime<Utc>>,
}

#[derive(Debug)]
struct JettonTransfer {
    hash: String,
    event_index: i64,
    direction: &'static str,
    amount: String,
    master: String,
    from: String,
    to: String,
    block_time: Option<DateTime<Utc>>,
}

struct StreamSync<T> {
    events: Vec<T>,
    newest_lt: Option<i64>,
    backfill_end_lt: Option<i64>,
    backfill_exhausted: bool,
    fetched: usize,
    pages: usize,
    may_have_more: bool,
}

impl TonCenterClient {
    pub fn from_env(client: Client) -> Result<Self, TonCenterConfigError> {
        let base_url = env::var("TONCENTER_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());
        let api_key = env::var("TONCENTER_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty());
        Self::new(&base_url, api_key.as_deref(), client)
    }

    pub fn new(
        base_url: &str,
        api_key: Option<&str>,
        client: Client,
    ) -> Result<Self, TonCenterConfigError> {
        let mut base_url =
            Url::parse(base_url.trim()).map_err(|_| TonCenterConfigError::InvalidBaseUrl)?;
        let host = base_url
            .host_str()
            .ok_or(TonCenterConfigError::InvalidBaseUrl)?;
        if !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(TonCenterConfigError::InvalidBaseUrl);
        }
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        if base_url.scheme() != "https" && !(base_url.scheme() == "http" && loopback) {
            return Err(TonCenterConfigError::InsecureBaseUrl);
        }
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        let api_key = api_key
            .map(|key| HeaderValue::try_from(key).map_err(|_| TonCenterConfigError::InvalidApiKey))
            .transpose()?;
        Ok(Self {
            client,
            base_url,
            api_key,
            unauthenticated_request_gate: Arc::new(Mutex::new(None)),
        })
    }

    pub fn is_configured(&self) -> bool {
        !self.base_url.as_str().is_empty()
    }

    fn source_host(&self) -> &str {
        self.base_url.host_str().unwrap_or("toncenter.com")
    }

    async fn get_page(
        &self,
        endpoint: &str,
        address: &str,
        start_lt: Option<i64>,
        end_lt: Option<i64>,
    ) -> Result<Vec<Value>, ApiError> {
        let url = self
            .base_url
            .join(endpoint)
            .map_err(|_| ApiError::ProviderUnavailable)?;
        let mut request = self.client.get(url).query(&[
            ("limit", PAGE_SIZE.to_string()),
            ("sort", sort_for(start_lt).into()),
        ]);
        request = if endpoint == "transactions" {
            request.query(&[("account", address)])
        } else {
            request.query(&[("owner_address", address)])
        };
        if let Some(start_lt) = start_lt {
            request = request.query(&[("start_lt", start_lt)]);
        }
        if let Some(end_lt) = end_lt {
            request = request.query(&[("end_lt", end_lt)]);
        }
        if let Some(api_key) = &self.api_key {
            request = request.header("X-API-Key", api_key.clone());
        }

        if self.api_key.is_none() {
            let mut last_request = self.unauthenticated_request_gate.lock().await;
            if let Some(previous) = *last_request {
                tokio::time::sleep(Duration::from_secs(1).saturating_sub(previous.elapsed())).await;
            }
            *last_request = Some(Instant::now());
        }
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
        let response: Value =
            serde_json::from_slice(&body).map_err(|_| ApiError::ProviderUnavailable)?;
        if response
            .get("code")
            .and_then(Value::as_i64)
            .is_some_and(|code| code != 0)
            || response.get("error").is_some()
        {
            return Err(ApiError::ProviderUnavailable);
        }
        let result_key = if endpoint == "transactions" {
            "transactions"
        } else {
            "jetton_transfers"
        };
        response
            .get(result_key)
            .and_then(Value::as_array)
            .cloned()
            .ok_or(ApiError::ProviderUnavailable)
    }
}

fn sort_for(start_lt: Option<i64>) -> &'static str {
    if start_lt.is_some() { "asc" } else { "desc" }
}

pub(crate) async fn sync_tracked_address(
    state: &ApiState,
    address_id: Uuid,
    address: &str,
) -> Result<SyncResult, ApiError> {
    let cursor = sqlx::query_as::<_, SyncCursor>(
        "SELECT native_newest_lt, native_backfill_end_lt, native_backfill_exhausted, \
         jetton_newest_lt, jetton_backfill_end_lt, jetton_backfill_exhausted \
         FROM ton_sync_cursors WHERE tracked_address_id = $1",
    )
    .bind(address_id)
    .fetch_optional(&state.database)
    .await?;

    let (native_head, native_backfill) = sync_native_stream(
        &state.ton_center,
        address,
        cursor.as_ref().and_then(|row| row.native_newest_lt),
        cursor.as_ref().and_then(|row| row.native_backfill_end_lt),
        cursor
            .as_ref()
            .is_some_and(|row| row.native_backfill_exhausted),
    )
    .await?;
    let (jetton_head, jetton_backfill) = sync_jetton_stream(
        &state.ton_center,
        address,
        cursor.as_ref().and_then(|row| row.jetton_newest_lt),
        cursor.as_ref().and_then(|row| row.jetton_backfill_end_lt),
        cursor
            .as_ref()
            .is_some_and(|row| row.jetton_backfill_exhausted),
    )
    .await?;

    let mut native_events = native_head.events;
    native_events.extend(native_backfill.events);
    let mut jetton_events = jetton_head.events;
    jetton_events.extend(jetton_backfill.events);
    let fetched = native_head.fetched
        + native_backfill.fetched
        + jetton_head.fetched
        + jetton_backfill.fetched;
    let pages =
        native_head.pages + native_backfill.pages + jetton_head.pages + jetton_backfill.pages;
    let may_have_more_history = native_head.may_have_more
        || native_backfill.may_have_more
        || jetton_head.may_have_more
        || jetton_backfill.may_have_more;
    let observed_at = Utc::now();
    let mut transaction = state.database.begin().await?;
    let mut saved = 0;
    for transfer in &native_events {
        let result = sqlx::query(
            "INSERT INTO observed_transfers \
             (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, \
              received_amount, spent_amount, asset, token_decimals, from_address, to_address, \
              block_height, block_time, confirmed, provider, source_host, observed_at) \
             VALUES ($1, 'ton', $2, $3, 'ton_native', $4, $5::NUMERIC, $6::NUMERIC, 'TON', 9, \
              $7, $8, $9, $10, TRUE, $11, $12, $13) \
             ON CONFLICT (tracked_address_id, transaction_hash, transfer_kind, event_index) \
             DO UPDATE SET direction = EXCLUDED.direction, received_amount = EXCLUDED.received_amount, \
              spent_amount = EXCLUDED.spent_amount, from_address = EXCLUDED.from_address, \
              to_address = EXCLUDED.to_address, block_height = EXCLUDED.block_height, \
              block_time = EXCLUDED.block_time, provider = EXCLUDED.provider, source_host = EXCLUDED.source_host, \
              observed_at = EXCLUDED.observed_at",
        )
        .bind(address_id)
        .bind(&transfer.hash)
        .bind(transfer.event_index)
        .bind(transfer.direction)
        .bind(&transfer.received)
        .bind(&transfer.spent)
        .bind(&transfer.from)
        .bind(&transfer.to)
        .bind(transfer.block_height)
        .bind(transfer.block_time)
        .bind(PROVIDER)
        .bind(state.ton_center.source_host())
        .bind(observed_at)
        .execute(&mut *transaction)
        .await?;
        saved += result.rows_affected() as usize;
    }
    for transfer in &jetton_events {
        let result = sqlx::query(
            "INSERT INTO observed_transfers \
             (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, \
              received_amount, spent_amount, asset, token_contract_address, from_address, to_address, \
              block_time, confirmed, provider, source_host, observed_at) \
             VALUES ($1, 'ton', $2, $3, 'ton_jetton', $4, $5::NUMERIC, $6::NUMERIC, 'JETTON', \
              $7, $8, $9, $10, TRUE, $11, $12, $13) \
             ON CONFLICT (tracked_address_id, transaction_hash, transfer_kind, event_index) \
             DO UPDATE SET direction = EXCLUDED.direction, received_amount = EXCLUDED.received_amount, \
              spent_amount = EXCLUDED.spent_amount, asset = EXCLUDED.asset, \
              token_contract_address = EXCLUDED.token_contract_address, from_address = EXCLUDED.from_address, \
              to_address = EXCLUDED.to_address, block_time = EXCLUDED.block_time, provider = EXCLUDED.provider, \
              source_host = EXCLUDED.source_host, observed_at = EXCLUDED.observed_at",
        )
        .bind(address_id)
        .bind(&transfer.hash)
        .bind(transfer.event_index)
        .bind(transfer.direction)
        .bind(if transfer.direction == "inbound" { &transfer.amount } else { "0" })
        .bind(if transfer.direction == "outbound" { &transfer.amount } else { "0" })
        .bind(&transfer.master)
        .bind(&transfer.from)
        .bind(&transfer.to)
        .bind(transfer.block_time)
        .bind(PROVIDER)
        .bind(state.ton_center.source_host())
        .bind(observed_at)
        .execute(&mut *transaction)
        .await?;
        saved += result.rows_affected() as usize;
    }

    sqlx::query(
        "INSERT INTO ton_sync_cursors \
         (tracked_address_id, native_newest_lt, native_backfill_end_lt, native_backfill_exhausted, \
          jetton_newest_lt, jetton_backfill_end_lt, jetton_backfill_exhausted, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (tracked_address_id) DO UPDATE SET native_newest_lt = EXCLUDED.native_newest_lt, \
          native_backfill_end_lt = EXCLUDED.native_backfill_end_lt, \
          native_backfill_exhausted = EXCLUDED.native_backfill_exhausted, \
          jetton_newest_lt = EXCLUDED.jetton_newest_lt, jetton_backfill_end_lt = EXCLUDED.jetton_backfill_end_lt, \
          jetton_backfill_exhausted = EXCLUDED.jetton_backfill_exhausted, updated_at = EXCLUDED.updated_at",
    )
    .bind(address_id)
    .bind(max_latest(cursor.as_ref().and_then(|row| row.native_newest_lt), native_head.newest_lt))
    .bind(native_backfill.backfill_end_lt)
    .bind(native_backfill.backfill_exhausted)
    .bind(max_latest(cursor.as_ref().and_then(|row| row.jetton_newest_lt), jetton_head.newest_lt))
    .bind(jetton_backfill.backfill_end_lt)
    .bind(jetton_backfill.backfill_exhausted)
    .bind(observed_at)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(SyncResult {
        provider: PROVIDER.into(),
        fetched,
        saved,
        pages,
        may_have_more_history,
        observed_at,
    })
}

async fn sync_native_stream(
    provider: &TonCenterClient,
    address: &str,
    newest_lt: Option<i64>,
    backfill_end_lt: Option<i64>,
    backfill_exhausted: bool,
) -> Result<(StreamSync<NativeTransfer>, StreamSync<NativeTransfer>), ApiError> {
    let head = provider
        .get_page(
            "transactions",
            address,
            newest_lt.and_then(|lt| lt.checked_add(1)),
            None,
        )
        .await?;
    let head_was_initial = newest_lt.is_none();
    let mut head_events = parse_native_events(&head, address);
    let head_newest = latest_native_lt(&head).or(newest_lt);
    let head_progress = StreamSync {
        events: Vec::new(),
        newest_lt: head_newest,
        backfill_end_lt,
        backfill_exhausted,
        fetched: head.len(),
        pages: 1,
        may_have_more: head.len() == PAGE_SIZE,
    };
    let mut backfill_progress = StreamSync {
        events: Vec::new(),
        newest_lt: head_newest,
        backfill_end_lt,
        backfill_exhausted,
        fetched: 0,
        pages: 0,
        may_have_more: !backfill_exhausted,
    };
    if head_was_initial {
        let oldest_lt = oldest_native_lt(&head);
        backfill_progress.backfill_end_lt = oldest_lt.and_then(|lt| lt.checked_sub(1));
        backfill_progress.backfill_exhausted = head.len() < PAGE_SIZE || oldest_lt.is_none();
        backfill_progress.may_have_more = !backfill_progress.backfill_exhausted;
    } else if !backfill_exhausted {
        if let Some(end_lt) = backfill_end_lt {
            let page = provider
                .get_page("transactions", address, None, Some(end_lt))
                .await?;
            backfill_progress.events = parse_native_events(&page, address);
            backfill_progress.fetched = page.len();
            backfill_progress.pages = 1;
            backfill_progress.may_have_more = page.len() == PAGE_SIZE;
            backfill_progress.backfill_end_lt =
                oldest_native_lt(&page).and_then(|lt| lt.checked_sub(1));
            backfill_progress.backfill_exhausted = page.len() < PAGE_SIZE || page.is_empty();
        }
    }
    head_events.shrink_to_fit();
    head_progress.events = head_events;
    Ok((head_progress, backfill_progress))
}

async fn sync_jetton_stream(
    provider: &TonCenterClient,
    address: &str,
    newest_lt: Option<i64>,
    backfill_end_lt: Option<i64>,
    backfill_exhausted: bool,
) -> Result<(StreamSync<JettonTransfer>, StreamSync<JettonTransfer>), ApiError> {
    let head = provider
        .get_page(
            "jetton/transfers",
            address,
            newest_lt.and_then(|lt| lt.checked_add(1)),
            None,
        )
        .await?;
    let head_was_initial = newest_lt.is_none();
    let head_events = parse_jetton_events(&head, address);
    let head_newest = latest_jetton_lt(&head).or(newest_lt);
    let mut head_progress = StreamSync {
        events: head_events,
        newest_lt: head_newest,
        backfill_end_lt,
        backfill_exhausted,
        fetched: head.len(),
        pages: 1,
        may_have_more: head.len() == PAGE_SIZE,
    };
    let mut backfill_progress = StreamSync {
        events: Vec::new(),
        newest_lt: head_newest,
        backfill_end_lt,
        backfill_exhausted,
        fetched: 0,
        pages: 0,
        may_have_more: !backfill_exhausted,
    };
    if head_was_initial {
        let oldest_lt = oldest_jetton_lt(&head);
        backfill_progress.backfill_end_lt = oldest_lt.and_then(|lt| lt.checked_sub(1));
        backfill_progress.backfill_exhausted = head.len() < PAGE_SIZE || oldest_lt.is_none();
        backfill_progress.may_have_more = !backfill_progress.backfill_exhausted;
    } else if !backfill_exhausted {
        if let Some(end_lt) = backfill_end_lt {
            let page = provider
                .get_page("jetton/transfers", address, None, Some(end_lt))
                .await?;
            backfill_progress.events = parse_jetton_events(&page, address);
            backfill_progress.fetched = page.len();
            backfill_progress.pages = 1;
            backfill_progress.may_have_more = page.len() == PAGE_SIZE;
            backfill_progress.backfill_end_lt =
                oldest_jetton_lt(&page).and_then(|lt| lt.checked_sub(1));
            backfill_progress.backfill_exhausted = page.len() < PAGE_SIZE || page.is_empty();
        }
    }
    Ok((head_progress, backfill_progress))
}

fn parse_native_events(rows: &[Value], tracked_address: &str) -> Vec<NativeTransfer> {
    let mut events = Vec::new();
    for transaction in rows {
        let Some(hash) = transaction
            .get("hash")
            .and_then(Value::as_str)
            .filter(|hash| is_hex_hash(hash))
        else {
            continue;
        };
        let Some(lt) = transaction.get("lt").and_then(parse_i64) else {
            continue;
        };
        let time = transaction
            .get("now")
            .and_then(parse_i64)
            .and_then(unix_time);
        let block_height = transaction
            .get("block_ref")
            .and_then(|block| block.get("seqno"))
            .and_then(parse_i64);
        if transaction
            .pointer("/description/aborted")
            .and_then(Value::as_bool)
            == Some(true)
        {
            continue;
        }
        if let Some(inbound) = transaction.get("in_msg") {
            if let Some(event) =
                native_message_event(inbound, tracked_address, hash, lt, -1, time, block_height)
            {
                events.push(event);
            }
        }
        if let Some(messages) = transaction.get("out_msgs").and_then(Value::as_array) {
            for (index, outbound) in messages.iter().enumerate() {
                let Some(index) = i64::try_from(index)
                    .ok()
                    .and_then(|index| index.checked_add(1))
                else {
                    continue;
                };
                if let Some(event) = native_message_event(
                    outbound,
                    tracked_address,
                    hash,
                    lt,
                    index,
                    time,
                    block_height,
                ) {
                    events.push(event);
                }
            }
        }
    }
    events
}

fn native_message_event(
    message: &Value,
    tracked_address: &str,
    hash: &str,
    lt: i64,
    event_index: i64,
    block_time: Option<DateTime<Utc>>,
    block_height: Option<i64>,
) -> Option<NativeTransfer> {
    let from = crate::addresses::normalize_ton_address(message.get("source")?.as_str()?).ok()?;
    let to = crate::addresses::normalize_ton_address(message.get("destination")?.as_str()?).ok()?;
    if message.get("bounced").and_then(Value::as_bool) == Some(true)
        || from == to
        || (from != tracked_address && to != tracked_address)
    {
        return None;
    }
    let amount = message.get("value").and_then(parse_u128)?.to_string();
    let direction = if from == tracked_address {
        "outbound"
    } else {
        "inbound"
    };
    Some(NativeTransfer {
        hash: hash.to_ascii_lowercase(),
        event_index,
        direction,
        received: if direction == "inbound" {
            amount.clone()
        } else {
            "0".into()
        },
        spent: if direction == "outbound" {
            amount
        } else {
            "0".into()
        },
        from,
        to,
        block_height,
        block_time,
    })
}

fn parse_jetton_events(rows: &[Value], tracked_address: &str) -> Vec<JettonTransfer> {
    rows.iter()
        .filter_map(|transfer| {
            if transfer.get("transaction_aborted").and_then(Value::as_bool) == Some(true) {
                return None;
            }
            let hash = transfer.get("transaction_hash")?.as_str()?;
            if !is_hex_hash(hash) {
                return None;
            }
            let lt = transfer.get("transaction_lt").and_then(parse_i64)?;
            let from =
                crate::addresses::normalize_ton_address(transfer.get("source")?.as_str()?).ok()?;
            let to =
                crate::addresses::normalize_ton_address(transfer.get("destination")?.as_str()?)
                    .ok()?;
            if from == to || (from != tracked_address && to != tracked_address) {
                return None;
            }
            let amount = transfer.get("amount").and_then(parse_jetton_amount)?;
            let master =
                crate::addresses::normalize_ton_address(transfer.get("jetton_master")?.as_str()?)
                    .ok()?;
            let query_id = transfer
                .get("query_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let event_index = stable_event_index(&[
                hash,
                &lt.to_string(),
                query_id,
                &from,
                &to,
                &master,
                &amount,
            ]);
            let direction = if from == tracked_address {
                "outbound"
            } else {
                "inbound"
            };
            Some(JettonTransfer {
                hash: hash.to_ascii_lowercase(),
                event_index,
                direction,
                amount,
                master,
                from,
                to,
                block_time: transfer
                    .get("transaction_now")
                    .and_then(parse_i64)
                    .and_then(unix_time),
            })
        })
        .collect()
}

fn latest_native_lt(rows: &[Value]) -> Option<i64> {
    rows.iter()
        .filter_map(|row| row.get("lt").and_then(parse_i64))
        .max()
}

fn oldest_native_lt(rows: &[Value]) -> Option<i64> {
    rows.iter()
        .filter_map(|row| row.get("lt").and_then(parse_i64))
        .min()
}

fn latest_jetton_lt(rows: &[Value]) -> Option<i64> {
    rows.iter()
        .filter_map(|row| row.get("transaction_lt").and_then(parse_i64))
        .max()
}

fn oldest_jetton_lt(rows: &[Value]) -> Option<i64> {
    rows.iter()
        .filter_map(|row| row.get("transaction_lt").and_then(parse_i64))
        .min()
}

fn parse_i64(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

fn parse_u128(value: &Value) -> Option<u128> {
    value
        .as_u64()
        .map(u128::from)
        .or_else(|| value.as_str()?.parse().ok())
}

fn parse_jetton_amount(value: &Value) -> Option<String> {
    let amount = value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|value| value.to_string()))?;
    if amount.is_empty() || amount.len() > 78 || !amount.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let normalized = amount.trim_start_matches('0');
    Some(
        if normalized.is_empty() {
            "0"
        } else {
            normalized
        }
        .to_owned(),
    )
}

fn unix_time(value: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(value, 0).single()
}

fn is_hex_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn stable_event_index(parts: &[&str]) -> i64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in parts.join("\0").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    (hash & i64::MAX as u64) as i64
}

fn max_latest(previous: Option<i64>, latest: Option<i64>) -> Option<i64> {
    match (previous, latest) {
        (Some(previous), Some(latest)) => Some(previous.max(latest)),
        (Some(previous), None) => Some(previous),
        (None, latest) => latest,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{parse_jetton_amount, parse_jetton_events};

    #[test]
    fn jetton_amount_keeps_full_uint256_precision() {
        let amount =
            "115792089237316195423570985008687907853269984665640564039457584007913129639935";
        assert_eq!(parse_jetton_amount(&json!(amount)).as_deref(), Some(amount));
        assert_eq!(parse_jetton_amount(&json!("00042")).as_deref(), Some("42"));
        assert_eq!(parse_jetton_amount(&json!("-1")), None);
        assert_eq!(parse_jetton_amount(&json!("9".repeat(79))), None);
    }

    #[test]
    fn ignores_failed_and_unrelated_jetton_rows() {
        let rows = vec![
            json!({"transaction_aborted":true}),
            json!({"source":"0:invalid"}),
        ];
        assert!(parse_jetton_events(&rows, &format!("0:{}", "00".repeat(32))).is_empty());
    }
}
