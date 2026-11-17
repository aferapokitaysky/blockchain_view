use std::{
    collections::BTreeMap,
    env,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::http::HeaderValue;
use chrono::{DateTime, Utc};
use redis::Client as RedisClient;
use reqwest::{Client, Url};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{ApiState, error::ApiError, transfers::SyncResult};

const DEFAULT_URL: &str = "https://graphql.mainnet.sui.io/graphql";
const PAGE_SIZE: usize = 50;
const MAX_PAGES_PER_SYNC: usize = 3;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const PROVIDER: &str = "Sui GraphQL RPC";
const REQUEST_GATE_KEY: &str = "tracefold:sui:graphql:request-gate";
const REQUEST_INTERVAL: Duration = Duration::from_millis(400);

#[derive(Clone)]
pub struct SuiClient {
    client: Client,
    endpoint: Url,
    api_key: Option<HeaderValue>,
    redis: Option<RedisClient>,
    local_request_gate: Arc<Mutex<Option<Instant>>>,
}

impl SuiClient {
    pub fn from_env(client: Client, redis: RedisClient) -> Result<Self, SuiConfigError> {
        let endpoint = env::var("SUI_GRAPHQL_URL").unwrap_or_else(|_| DEFAULT_URL.into());
        let api_key = env::var("SUI_GRAPHQL_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty());
        Self::new(&endpoint, api_key.as_deref(), client, Some(redis))
    }

    pub fn new(
        endpoint: &str,
        api_key: Option<&str>,
        client: Client,
        redis: Option<RedisClient>,
    ) -> Result<Self, SuiConfigError> {
        let endpoint = Url::parse(endpoint.trim()).map_err(|_| SuiConfigError::InvalidUrl)?;
        if endpoint.scheme() != "https"
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(SuiConfigError::InvalidUrl);
        }
        let api_key = api_key
            .map(|key| HeaderValue::try_from(key).map_err(|_| SuiConfigError::InvalidApiKey))
            .transpose()?;
        Ok(Self {
            client,
            endpoint,
            api_key,
            redis,
            local_request_gate: Arc::new(Mutex::new(None)),
        })
    }

    pub fn is_configured(&self) -> bool {
        true
    }

    async fn query(&self, query: &str, variables: Value) -> Result<Value, ApiError> {
        self.pace_request().await?;
        let mut request = self
            .client
            .post(self.endpoint.clone())
            .json(&json!({"query": query, "variables": variables}));
        if let Some(key) = &self.api_key {
            request = request.header(
                "Authorization",
                format!(
                    "Bearer {}",
                    key.to_str().map_err(|_| ApiError::ProviderUnavailable)?
                ),
            );
        }
        let response = request
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
        let mut response = response;
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
        let value: Value =
            serde_json::from_slice(&body).map_err(|_| ApiError::ProviderUnavailable)?;
        if value
            .get("errors")
            .and_then(Value::as_array)
            .is_some_and(|errors| !errors.is_empty())
        {
            return Err(ApiError::ProviderUnavailable);
        }
        value
            .get("data")
            .cloned()
            .ok_or(ApiError::ProviderUnavailable)
    }

    async fn pace_request(&self) -> Result<(), ApiError> {
        let Some(redis) = &self.redis else {
            pace_local(&self.local_request_gate).await;
            return Ok(());
        };
        loop {
            let mut connection = redis
                .get_multiplexed_async_connection()
                .await
                .map_err(|_| ApiError::ProviderUnavailable)?;
            let acquired: Option<String> = redis::cmd("SET")
                .arg(REQUEST_GATE_KEY)
                .arg("1")
                .arg("NX")
                .arg("PX")
                .arg(REQUEST_INTERVAL.as_millis() as u64)
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
}

async fn pace_local(gate: &Mutex<Option<Instant>>) {
    let mut last_request = gate.lock().await;
    if let Some(last_request) = *last_request {
        let wait = REQUEST_INTERVAL.saturating_sub(last_request.elapsed());
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
    *last_request = Some(Instant::now());
}

#[derive(Debug, thiserror::Error)]
pub enum SuiConfigError {
    #[error("SUI_GRAPHQL_URL must be a valid HTTPS URL")]
    InvalidUrl,
    #[error("SUI_GRAPHQL_API_KEY contains invalid header characters")]
    InvalidApiKey,
}

#[derive(Debug, PartialEq, Eq)]
struct CoinDelta {
    owner: String,
    coin_type: String,
    amount: i128,
}

#[derive(Debug, Default, sqlx::FromRow)]
struct SuiCursorState {
    head_cursor: Option<String>,
    backfill_cursor: Option<String>,
    backfill_exhausted: bool,
}

#[derive(Debug)]
struct SuiPage {
    transactions: Vec<Value>,
    start_cursor: Option<String>,
    end_cursor: Option<String>,
    has_previous_page: bool,
    has_next_page: bool,
}

#[derive(Default)]
struct SuiSyncStats {
    fetched: usize,
    saved: usize,
}

pub(crate) async fn sync_tracked_address(
    state: &ApiState,
    address_id: Uuid,
    address: &str,
) -> Result<SyncResult, ApiError> {
    const QUERY: &str = "query AddressTransactions($address: SuiAddress!, $first: Int, $after: String, $last: Int, $before: String) { transactions(first: $first, after: $after, last: $last, before: $before, filter: { affectedAddress: $address }) { pageInfo { hasPreviousPage hasNextPage startCursor endCursor } nodes { digest effects { status timestamp checkpoint { sequenceNumber } balanceChangesJson } } } }";
    let address =
        normalize_sui_address(address).ok_or(ApiError::Validation("Неверный формат адреса Sui"))?;
    let mut cursors = sqlx::query_as::<_, SuiCursorState>(
        "SELECT head_cursor, backfill_cursor, backfill_exhausted FROM sui_sync_cursors WHERE tracked_address_id = $1",
    )
    .bind(address_id)
    .fetch_optional(&state.database)
    .await?
    .unwrap_or_default();
    let mut stats = SuiSyncStats::default();
    let mut pages = 0;
    let mut head_caught_up = false;
    let observed_at = Utc::now();
    let source_host = state
        .sui
        .endpoint
        .host_str()
        .unwrap_or("graphql.mainnet.sui.io")
        .to_owned();

    if cursors.head_cursor.is_none() {
        let page = read_page(state, QUERY, &address, None, None, Some(PAGE_SIZE), None).await?;
        save_page(
            state,
            address_id,
            &address,
            &page,
            &source_host,
            observed_at,
            &mut stats,
        )
        .await?;
        apply_initial_head_page(&mut cursors, &page)?;
        pages += 1;
        head_caught_up = !page.has_next_page;
        persist_cursors(state, address_id, &cursors, observed_at).await?;
    } else {
        while pages < MAX_PAGES_PER_SYNC {
            let page = read_page(
                state,
                QUERY,
                &address,
                Some(PAGE_SIZE),
                cursors.head_cursor.as_deref(),
                None,
                None,
            )
            .await?;
            save_page(
                state,
                address_id,
                &address,
                &page,
                &source_host,
                observed_at,
                &mut stats,
            )
            .await?;
            apply_newer_head_page(&mut cursors, &page);
            pages += 1;
            head_caught_up = !page.has_next_page;
            persist_cursors(state, address_id, &cursors, observed_at).await?;
            if head_caught_up {
                break;
            }
        }
    }

    while head_caught_up && pages < MAX_PAGES_PER_SYNC && !cursors.backfill_exhausted {
        let Some(before) = cursors.backfill_cursor.clone() else {
            cursors.backfill_exhausted = true;
            persist_cursors(state, address_id, &cursors, observed_at).await?;
            break;
        };
        let page = read_page(
            state,
            QUERY,
            &address,
            None,
            None,
            Some(PAGE_SIZE),
            Some(&before),
        )
        .await?;
        save_page(
            state,
            address_id,
            &address,
            &page,
            &source_host,
            observed_at,
            &mut stats,
        )
        .await?;
        apply_backfill_page(&mut cursors, &page)?;
        pages += 1;
        persist_cursors(state, address_id, &cursors, observed_at).await?;
    }

    Ok(SyncResult {
        provider: PROVIDER.into(),
        fetched: stats.fetched,
        saved: stats.saved,
        pages,
        may_have_more_history: !head_caught_up || !cursors.backfill_exhausted,
        observed_at,
    })
}

async fn read_page(
    state: &ApiState,
    query: &str,
    address: &str,
    first: Option<usize>,
    after: Option<&str>,
    last: Option<usize>,
    before: Option<&str>,
) -> Result<SuiPage, ApiError> {
    let data = state
        .sui
        .query(
            query,
            json!({
                "address": address,
                "first": first,
                "after": after,
                "last": last,
                "before": before,
            }),
        )
        .await?;
    let connection = data
        .get("transactions")
        .ok_or(ApiError::ProviderUnavailable)?;
    let transactions = connection
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or(ApiError::ProviderUnavailable)?;
    if transactions.len() > PAGE_SIZE {
        return Err(ApiError::ProviderUnavailable);
    }
    let page_info = connection
        .get("pageInfo")
        .ok_or(ApiError::ProviderUnavailable)?;
    let cursor = |key: &str| -> Result<Option<String>, ApiError> {
        let value = page_info.get(key).ok_or(ApiError::ProviderUnavailable)?;
        let Some(value) = value.as_str() else {
            if value.is_null() {
                return Ok(None);
            }
            return Err(ApiError::ProviderUnavailable);
        };
        if value.len() > 8192 {
            return Err(ApiError::ProviderUnavailable);
        }
        Ok(Some(value.to_owned()))
    };
    let has_previous_page = page_info
        .get("hasPreviousPage")
        .and_then(Value::as_bool)
        .ok_or(ApiError::ProviderUnavailable)?;
    let has_next_page = page_info
        .get("hasNextPage")
        .and_then(Value::as_bool)
        .ok_or(ApiError::ProviderUnavailable)?;
    let start_cursor = cursor("startCursor")?;
    let end_cursor = cursor("endCursor")?;
    if (has_previous_page && start_cursor.is_none()) || (has_next_page && end_cursor.is_none()) {
        return Err(ApiError::ProviderUnavailable);
    }
    Ok(SuiPage {
        transactions: transactions.clone(),
        start_cursor,
        end_cursor,
        has_previous_page,
        has_next_page,
    })
}

async fn save_page(
    state: &ApiState,
    address_id: Uuid,
    address: &str,
    page: &SuiPage,
    source_host: &str,
    observed_at: DateTime<Utc>,
    stats: &mut SuiSyncStats,
) -> Result<(), ApiError> {
    for transaction in &page.transactions {
        stats.fetched += 1;
        stats.saved += save_transaction(
            state,
            address_id,
            address,
            transaction,
            source_host,
            observed_at,
        )
        .await?;
    }
    Ok(())
}

fn apply_initial_head_page(state: &mut SuiCursorState, page: &SuiPage) -> Result<(), ApiError> {
    state.head_cursor = page.end_cursor.clone();
    state.backfill_exhausted = !page.has_previous_page;
    state.backfill_cursor = if page.has_previous_page {
        Some(
            page.start_cursor
                .clone()
                .ok_or(ApiError::ProviderUnavailable)?,
        )
    } else {
        None
    };
    Ok(())
}

fn apply_backfill_page(state: &mut SuiCursorState, page: &SuiPage) -> Result<(), ApiError> {
    state.backfill_exhausted = !page.has_previous_page;
    state.backfill_cursor = if page.has_previous_page {
        Some(
            page.start_cursor
                .clone()
                .ok_or(ApiError::ProviderUnavailable)?,
        )
    } else {
        None
    };
    Ok(())
}

fn apply_newer_head_page(state: &mut SuiCursorState, page: &SuiPage) {
    if !page.transactions.is_empty() {
        state.head_cursor = page.end_cursor.clone();
    }
}

async fn save_transaction(
    state: &ApiState,
    tracked_address_id: Uuid,
    tracked_address: &str,
    transaction: &Value,
    source_host: &str,
    observed_at: DateTime<Utc>,
) -> Result<usize, ApiError> {
    let Some(digest) = transaction.get("digest").and_then(Value::as_str) else {
        return Err(ApiError::ProviderUnavailable);
    };
    let Some(effects) = transaction.get("effects") else {
        return Err(ApiError::ProviderUnavailable);
    };
    if effects.get("status").and_then(Value::as_str) != Some("SUCCESS") {
        return Ok(0);
    }
    let (changes, attribution_complete) = parse_balance_changes(effects.get("balanceChangesJson"))
        .ok_or(ApiError::ProviderUnavailable)?;
    let checkpoint = effects
        .get("checkpoint")
        .and_then(|checkpoint| checkpoint.get("sequenceNumber"))
        .and_then(parse_i64);
    let block_time = effects
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.with_timezone(&Utc));

    let mut grouped = BTreeMap::<(String, String), i128>::new();
    for change in changes {
        *grouped.entry((change.coin_type, change.owner)).or_default() += change.amount;
    }
    let coin_deltas = grouped
        .into_iter()
        .filter_map(|((coin_type, owner), amount)| {
            (amount != 0).then_some(CoinDelta {
                owner,
                coin_type,
                amount,
            })
        })
        .collect::<Vec<_>>();
    let mut tracked_changes = BTreeMap::<String, i128>::new();
    for delta in &coin_deltas {
        if delta.owner == tracked_address {
            *tracked_changes.entry(delta.coin_type.clone()).or_default() += delta.amount;
        }
    }

    let mut saved = 0;
    for (event_index, (coin_type, amount)) in tracked_changes.into_iter().enumerate() {
        if amount == 0 {
            continue;
        }
        let direction = if amount < 0 { "outbound" } else { "inbound" };
        let magnitude = amount.unsigned_abs().to_string();
        let (received, spent) = if amount < 0 {
            ("0", magnitude.as_str())
        } else {
            (magnitude.as_str(), "0")
        };
        let result = sqlx::query(
            "INSERT INTO observed_transfers (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, received_amount, spent_amount, asset, token_contract_address, from_address, to_address, block_height, block_time, confirmed, provider, source_host, observed_at) \
             VALUES ($1, 'sui', $2, $3, 'sui_balance_change', $4, $5, $6, $7, NULL, NULL, NULL, $8, $9, TRUE, $10, $11, $12) \
             ON CONFLICT (tracked_address_id, transaction_hash, transfer_kind, event_index) DO UPDATE SET direction = EXCLUDED.direction, received_amount = EXCLUDED.received_amount, spent_amount = EXCLUDED.spent_amount, asset = EXCLUDED.asset, block_height = EXCLUDED.block_height, block_time = EXCLUDED.block_time, provider = EXCLUDED.provider, source_host = EXCLUDED.source_host, observed_at = EXCLUDED.observed_at",
        )
        .bind(tracked_address_id)
        .bind(digest)
        .bind(event_index as i64)
        .bind(direction)
        .bind(received)
        .bind(spent)
        .bind(&coin_type)
        .bind(checkpoint)
        .bind(block_time)
        .bind(PROVIDER)
        .bind(source_host)
        .bind(observed_at)
        .execute(&state.database)
        .await?;
        saved += result.rows_affected() as usize;

        if let Some(counterparty) = unique_counterparty(
            &coin_deltas,
            attribution_complete,
            tracked_address,
            &coin_type,
            amount,
        ) {
            let participant_role = if amount < 0 { "input" } else { "output" };
            sqlx::query(
                "INSERT INTO observed_relationships (tracked_address_id, network, transaction_hash, counterparty_address, relationship_type, participant_role, block_height, block_time, confirmed, provider, source_host, observed_at) \
                 VALUES ($1, 'sui', $2, $3, 'same_transaction', $4, $5, $6, TRUE, $7, $8, $9) \
                 ON CONFLICT (tracked_address_id, transaction_hash, counterparty_address, relationship_type) DO UPDATE SET participant_role = EXCLUDED.participant_role, block_height = EXCLUDED.block_height, block_time = EXCLUDED.block_time, provider = EXCLUDED.provider, source_host = EXCLUDED.source_host, observed_at = EXCLUDED.observed_at",
            )
            .bind(tracked_address_id)
            .bind(digest)
            .bind(counterparty)
            .bind(participant_role)
            .bind(checkpoint)
            .bind(block_time)
            .bind(PROVIDER)
            .bind(source_host)
            .bind(observed_at)
            .execute(&state.database)
            .await?;
        }
    }
    Ok(saved)
}

async fn persist_cursors(
    state: &ApiState,
    address_id: Uuid,
    cursors: &SuiCursorState,
    updated_at: DateTime<Utc>,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO sui_sync_cursors (tracked_address_id, head_cursor, backfill_cursor, backfill_exhausted, updated_at) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (tracked_address_id) DO UPDATE SET head_cursor = EXCLUDED.head_cursor, backfill_cursor = EXCLUDED.backfill_cursor, backfill_exhausted = EXCLUDED.backfill_exhausted, updated_at = EXCLUDED.updated_at",
    )
    .bind(address_id)
    .bind(&cursors.head_cursor)
    .bind(&cursors.backfill_cursor)
    .bind(cursors.backfill_exhausted)
    .bind(updated_at)
    .execute(&state.database)
    .await?;
    Ok(())
}

fn parse_balance_changes(value: Option<&Value>) -> Option<(Vec<CoinDelta>, bool)> {
    let changes = value?.as_array()?;
    let mut parsed = Vec::with_capacity(changes.len());
    let mut complete = true;
    for change in changes {
        let parsed_change = change
            .get("address")
            .and_then(Value::as_str)
            .and_then(normalize_sui_address)
            .zip(
                change
                    .get("coinType")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            )
            .zip(change.get("amount").and_then(parse_i128))
            .map(|((owner, coin_type), amount)| CoinDelta {
                owner,
                coin_type,
                amount,
            });
        if let Some(parsed_change) = parsed_change {
            parsed.push(parsed_change);
        } else {
            complete = false;
        }
    }
    Some((parsed, complete))
}

fn unique_counterparty<'a>(
    changes: &'a [CoinDelta],
    attribution_complete: bool,
    tracked_address: &str,
    coin_type: &str,
    tracked_amount: i128,
) -> Option<&'a str> {
    if !attribution_complete {
        return None;
    }
    let entries = changes
        .iter()
        .filter(|change| change.coin_type == coin_type && change.amount != 0)
        .collect::<Vec<_>>();
    if entries.len() != 2 {
        return None;
    }
    let tracked = entries
        .iter()
        .find(|change| change.owner == tracked_address && change.amount == tracked_amount)?;
    let counterparty = entries
        .iter()
        .find(|change| change.owner != tracked.owner)?;
    (tracked.amount.checked_add(counterparty.amount) == Some(0))
        .then_some(counterparty.owner.as_str())
}

pub(crate) fn normalize_sui_address(value: &str) -> Option<String> {
    let value = value.trim();
    let hex = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    if hex.is_empty() || hex.len() > 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("0x{:0>64}", hex.to_ascii_lowercase()))
}

fn parse_i128(value: &Value) -> Option<i128> {
    value
        .as_i64()
        .map(i128::from)
        .or_else(|| value.as_str()?.parse().ok())
}

fn parse_i64(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::{
        CoinDelta, SuiCursorState, SuiPage, apply_backfill_page, apply_initial_head_page,
        apply_newer_head_page, normalize_sui_address, parse_balance_changes, unique_counterparty,
    };
    use serde_json::json;

    #[test]
    fn normalizes_sui_addresses_to_full_lowercase_hex() {
        assert_eq!(
            normalize_sui_address("0xAbC").unwrap(),
            format!("0x{:0>64}", "abc")
        );
        assert!(normalize_sui_address("0x").is_none());
        assert!(normalize_sui_address(&format!("0x{}g", "1".repeat(63))).is_none());
        assert!(normalize_sui_address(&format!("0x{}", "1".repeat(65))).is_none());
    }

    #[test]
    fn parses_json_balance_changes_and_preserves_large_signed_units() {
        let (changes, complete) = parse_balance_changes(Some(&json!([
            {"address":"0x1","coinType":"0x2::sui::SUI","amount":"-18446744073709551615"},
            {"address":"0x2","coinType":"0x2::sui::SUI","amount":"18446744073709551615"}
        ])))
        .unwrap();
        assert_eq!(changes.len(), 2);
        assert!(complete);
        assert_eq!(changes[0].amount, -18_446_744_073_709_551_615_i128);
    }

    #[test]
    fn malformed_balance_change_prevents_counterparty_inference() {
        let (changes, complete) = parse_balance_changes(Some(&json!([
            {"address":"0x1","coinType":"0x2::sui::SUI","amount":"-500"},
            {"address":"0x2","coinType":"0x2::sui::SUI","amount":"500"},
            {"address":"not-an-address","coinType":"0x2::sui::SUI","amount":"99"}
        ])))
        .unwrap();
        assert!(!complete);
        assert_eq!(changes.len(), 2);
        assert_eq!(
            unique_counterparty(
                &changes,
                complete,
                &normalize_sui_address("0x1").unwrap(),
                "0x2::sui::SUI",
                -500,
            ),
            None
        );
    }

    #[test]
    fn counterparty_requires_exactly_two_equal_opposite_owner_deltas() {
        let tracked = format!("0x{:0>64}", "1");
        let counterparty = format!("0x{:0>64}", "2");
        let changes = vec![
            CoinDelta {
                owner: tracked.clone(),
                coin_type: "0x2::sui::SUI".into(),
                amount: -500,
            },
            CoinDelta {
                owner: counterparty.clone(),
                coin_type: "0x2::sui::SUI".into(),
                amount: 500,
            },
        ];
        assert_eq!(
            unique_counterparty(&changes, true, &tracked, "0x2::sui::SUI", -500),
            Some(counterparty.as_str())
        );
        let mut ambiguous = changes;
        ambiguous.push(CoinDelta {
            owner: format!("0x{:0>64}", "3"),
            coin_type: "0x2::sui::SUI".into(),
            amount: 10,
        });
        assert_eq!(
            unique_counterparty(&ambiguous, true, &tracked, "0x2::sui::SUI", -500),
            None
        );
        assert_eq!(
            unique_counterparty(
                &[
                    CoinDelta {
                        owner: tracked.clone(),
                        coin_type: "0x2::sui::SUI".into(),
                        amount: -500
                    },
                    CoinDelta {
                        owner: counterparty,
                        coin_type: "0x2::sui::SUI".into(),
                        amount: 499
                    },
                ],
                true,
                &tracked,
                "0x2::sui::SUI",
                -500
            ),
            None
        );
    }

    #[test]
    fn head_and_backfill_cursors_advance_independently() {
        let mut cursors = SuiCursorState::default();
        let initial_head = SuiPage {
            transactions: vec![json!({"digest":"newest"})],
            start_cursor: Some("head-start".into()),
            end_cursor: Some("head-end".into()),
            has_previous_page: true,
            has_next_page: false,
        };
        apply_initial_head_page(&mut cursors, &initial_head).unwrap();
        assert_eq!(cursors.head_cursor.as_deref(), Some("head-end"));
        assert_eq!(cursors.backfill_cursor.as_deref(), Some("head-start"));
        assert!(!cursors.backfill_exhausted);

        let next_head = SuiPage {
            transactions: vec![json!({"digest":"newer"})],
            start_cursor: Some("newer-start".into()),
            end_cursor: Some("newer-end".into()),
            has_previous_page: false,
            has_next_page: false,
        };
        apply_newer_head_page(&mut cursors, &next_head);
        assert_eq!(cursors.head_cursor.as_deref(), Some("newer-end"));
        assert_eq!(cursors.backfill_cursor.as_deref(), Some("head-start"));

        let older_page = SuiPage {
            transactions: vec![json!({"digest":"older"})],
            start_cursor: Some("older-start".into()),
            end_cursor: Some("older-end".into()),
            has_previous_page: true,
            has_next_page: false,
        };
        apply_backfill_page(&mut cursors, &older_page).unwrap();
        assert_eq!(cursors.head_cursor.as_deref(), Some("newer-end"));
        assert_eq!(cursors.backfill_cursor.as_deref(), Some("older-start"));
    }
}
