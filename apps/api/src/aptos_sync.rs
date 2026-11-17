use std::env;

use axum::http::HeaderValue;
use chrono::{DateTime, Utc};
use reqwest::{Client, Url};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{ApiState, error::ApiError, transfers::SyncResult};

const DEFAULT_URL: &str = "https://api.mainnet.aptoslabs.com/v1/graphql";
const PAGE_SIZE: usize = 100;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const PROVIDER: &str = "Aptos Indexer GraphQL";

#[derive(Clone)]
pub struct AptosClient {
    client: Client,
    endpoint: Url,
    api_key: Option<HeaderValue>,
}

impl AptosClient {
    pub fn from_env(client: Client) -> Result<Self, AptosConfigError> {
        let endpoint = env::var("APTOS_INDEXER_URL").unwrap_or_else(|_| DEFAULT_URL.into());
        let api_key = env::var("APTOS_INDEXER_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty());
        Self::new(&endpoint, api_key.as_deref(), client)
    }

    pub fn new(
        endpoint: &str,
        api_key: Option<&str>,
        client: Client,
    ) -> Result<Self, AptosConfigError> {
        let endpoint = Url::parse(endpoint.trim()).map_err(|_| AptosConfigError::InvalidUrl)?;
        if endpoint.scheme() != "https"
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(AptosConfigError::InvalidUrl);
        }
        let api_key = api_key
            .map(|key| HeaderValue::try_from(key).map_err(|_| AptosConfigError::InvalidApiKey))
            .transpose()?;
        Ok(Self {
            client,
            endpoint,
            api_key,
        })
    }

    pub fn is_configured(&self) -> bool {
        true
    }

    async fn query(&self, query: &str, variables: Value) -> Result<Value, ApiError> {
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
}

#[derive(Debug, thiserror::Error)]
pub enum AptosConfigError {
    #[error("APTOS_INDEXER_URL must be a valid HTTPS URL")]
    InvalidUrl,
    #[error("APTOS_INDEXER_API_KEY contains invalid header characters")]
    InvalidApiKey,
}

#[derive(Debug)]
struct Activity {
    owner: String,
    asset: String,
    amount: String,
    event_type: String,
    event_index: i64,
    version: i64,
    timestamp: Option<DateTime<Utc>>,
}

pub(crate) async fn sync_tracked_address(
    state: &ApiState,
    address_id: Uuid,
    address: &str,
) -> Result<SyncResult, ApiError> {
    const ACCOUNT_QUERY: &str = "query Account($address: String!, $limit: Int!, $offset: Int!) { account_transactions(where: {account_address: {_eq: $address}}, order_by: {transaction_version: desc}, limit: $limit, offset: $offset) { transaction_version } }";
    const ACTIVITIES_QUERY: &str = "query Activities($versions: [bigint!]!) { fungible_asset_activities(where: {transaction_version: {_in: $versions}}, order_by: [{transaction_version: asc}, {event_index: asc}]) { owner_address asset_type amount type event_index transaction_version transaction_timestamp is_transaction_success } }";
    let offset: i32 = sqlx::query_scalar(
        "SELECT next_offset::int4 FROM aptos_sync_cursors WHERE tracked_address_id = $1",
    )
    .bind(address_id)
    .fetch_optional(&state.database)
    .await?
    .unwrap_or(0);
    let data = state
        .aptos
        .query(
            ACCOUNT_QUERY,
            json!({"address": address, "limit": PAGE_SIZE, "offset": offset}),
        )
        .await?;
    let rows = data
        .get("account_transactions")
        .and_then(Value::as_array)
        .ok_or(ApiError::ProviderUnavailable)?;
    let versions: Vec<i64> = rows
        .iter()
        .filter_map(|row| row.get("transaction_version").and_then(parse_i64))
        .collect();
    let fetched = rows.len();
    let activity_data = if versions.is_empty() {
        json!({"fungible_asset_activities": []})
    } else {
        state
            .aptos
            .query(ACTIVITIES_QUERY, json!({"versions": versions}))
            .await?
    };
    let activities = activity_data
        .get("fungible_asset_activities")
        .and_then(Value::as_array)
        .ok_or(ApiError::ProviderUnavailable)?
        .iter()
        .filter_map(parse_activity)
        .collect::<Vec<_>>();
    let observed_at = Utc::now();
    let mut saved = 0;
    for (index, activity) in activities
        .iter()
        .enumerate()
        .filter(|(_, activity)| activity.owner.eq_ignore_ascii_case(address))
    {
        let direction = activity_direction(&activity.event_type);
        let Some(direction) = direction else {
            continue;
        };
        let counterparties = activities
            .iter()
            .enumerate()
            .filter(|(other_index, other)| {
                *other_index != index
                    && other.version == activity.version
                    && other.asset == activity.asset
                    && other.amount == activity.amount
                    && activity_direction(&other.event_type)
                        .is_some_and(|other_direction| other_direction != direction)
            })
            .map(|(_, other)| other.owner.as_str())
            .filter(|owner| !owner.eq_ignore_ascii_case(address))
            .collect::<Vec<_>>();
        let counterparty = (counterparties.len() == 1).then_some(counterparties[0]);
        let (received, spent) = if direction == "outbound" {
            ("0", activity.amount.as_str())
        } else {
            (activity.amount.as_str(), "0")
        };
        let result = sqlx::query("INSERT INTO observed_transfers (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, received_amount, spent_amount, asset, token_contract_address, from_address, to_address, block_height, block_time, confirmed, provider, source_host, observed_at) VALUES ($1, 'aptos', $2, $3, 'aptos_fungible', $4, $5, $6, $7, $8, $9, $10, NULL, $11, TRUE, $12, 'api.mainnet.aptoslabs.com', $13) ON CONFLICT (tracked_address_id, transaction_hash, transfer_kind, event_index) DO UPDATE SET direction = EXCLUDED.direction, received_amount = EXCLUDED.received_amount, spent_amount = EXCLUDED.spent_amount, asset = EXCLUDED.asset, token_contract_address = EXCLUDED.token_contract_address, from_address = EXCLUDED.from_address, to_address = EXCLUDED.to_address, block_time = EXCLUDED.block_time, provider = EXCLUDED.provider, source_host = EXCLUDED.source_host, observed_at = EXCLUDED.observed_at")
            .bind(address_id).bind(activity.version.to_string()).bind(activity.event_index).bind(direction).bind(received).bind(spent).bind(&activity.asset).bind(&activity.asset)
            .bind(if direction == "outbound" { Some(address) } else { counterparty }).bind(if direction == "inbound" { Some(address) } else { counterparty })
            .bind(activity.timestamp).bind(PROVIDER).bind(observed_at).execute(&state.database).await?;
        saved += result.rows_affected() as usize;
    }
    let next_offset = if fetched == PAGE_SIZE && offset < 10_000_000 {
        offset + fetched as i32
    } else {
        0
    };
    sqlx::query("INSERT INTO aptos_sync_cursors (tracked_address_id, next_offset, updated_at) VALUES ($1, $2, $3) ON CONFLICT (tracked_address_id) DO UPDATE SET next_offset = EXCLUDED.next_offset, updated_at = EXCLUDED.updated_at")
        .bind(address_id).bind(next_offset as i64).bind(observed_at).execute(&state.database).await?;
    Ok(SyncResult {
        provider: PROVIDER.into(),
        fetched,
        saved,
        pages: usize::from(fetched > 0),
        may_have_more_history: fetched == PAGE_SIZE,
        observed_at,
    })
}

fn parse_i64(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

fn parse_activity(value: &Value) -> Option<Activity> {
    if value.get("is_transaction_success").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    Some(Activity {
        owner: value.get("owner_address")?.as_str()?.to_owned(),
        asset: value.get("asset_type")?.as_str()?.to_owned(),
        amount: value
            .get("amount")?
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                value
                    .get("amount")?
                    .as_i64()
                    .map(|amount| amount.to_string())
            })?,
        event_type: value.get("type")?.as_str()?.to_owned(),
        event_index: parse_i64(value.get("event_index")?)?,
        version: parse_i64(value.get("transaction_version")?)?,
        timestamp: value
            .get("transaction_timestamp")
            .and_then(Value::as_str)
            .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
            .map(|time| time.with_timezone(&Utc)),
    })
}

fn activity_direction(event_type: &str) -> Option<&'static str> {
    if event_type.ends_with("::withdraw") || event_type.ends_with("::Withdraw") {
        Some("outbound")
    } else if event_type.ends_with("::deposit") || event_type.ends_with("::Deposit") {
        Some("inbound")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{activity_direction, parse_activity};
    use serde_json::json;

    #[test]
    fn parses_fungible_asset_activity_without_losing_raw_amount() {
        let activity = parse_activity(&json!({"owner_address":"0x1","asset_type":"0x1::aptos_coin::AptosCoin","amount":"123456789","type":"0x1::fungible_asset::Withdraw","event_index":3,"transaction_version":"42","transaction_timestamp":"2025-01-02T03:04:05"})).unwrap();
        assert_eq!(activity.amount, "123456789");
        assert_eq!(
            activity_direction(&activity.event_type).unwrap(),
            "outbound"
        );
        assert_eq!(activity.version, 42);
    }

    #[test]
    fn ignores_unknown_activity_types_and_failed_transactions() {
        assert!(activity_direction("0x1::fungible_asset::Mint").is_none());
        assert!(parse_activity(&json!({"owner_address":"0x1","asset_type":"coin","amount":"1","type":"deposit","event_index":0,"transaction_version":1,"is_transaction_success":false})).is_none());
    }
}
