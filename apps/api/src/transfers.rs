use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{ApiState, error::ApiError};

const MAX_CONFIRMED_PAGES: usize = 20;

#[derive(Debug, Deserialize)]
struct EsploraTransaction {
    txid: String,
    #[serde(default)]
    vin: Vec<EsploraInput>,
    #[serde(default)]
    vout: Vec<EsploraOutput>,
    status: EsploraStatus,
}

#[derive(Debug, Deserialize)]
struct EsploraInput {
    prevout: Option<EsploraOutput>,
}

#[derive(Debug, Deserialize)]
struct EsploraOutput {
    scriptpubkey_address: Option<String>,
    value: u64,
}

#[derive(Debug, Default, Deserialize)]
struct EsploraStatus {
    confirmed: bool,
    block_height: Option<i64>,
    block_time: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SyncResult {
    pub provider: String,
    pub fetched: usize,
    pub saved: usize,
    pub pages: usize,
    pub may_have_more_history: bool,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct ObservedTransfer {
    pub id: Uuid,
    pub tracked_address_id: Uuid,
    pub network: String,
    pub transaction_hash: String,
    pub event_index: i64,
    pub transfer_kind: String,
    pub direction: String,
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
    pub provider: String,
    pub source_host: String,
    pub observed_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct AddressRow {
    network: String,
    address: String,
}

pub(crate) async fn sync_address(
    State(state): State<ApiState>,
    Path((investigation_id, address_id)): Path<(Uuid, Uuid)>,
) -> Result<(StatusCode, Json<SyncResult>), ApiError> {
    let result = sync_tracked_address(&state, investigation_id, address_id).await?;
    Ok((StatusCode::OK, Json(result)))
}

pub(crate) async fn sync_tracked_address(
    state: &ApiState,
    investigation_id: Uuid,
    address_id: Uuid,
) -> Result<SyncResult, ApiError> {
    let address = sqlx::query_as::<_, AddressRow>(
        "SELECT network, address FROM tracked_addresses WHERE id = $1 AND investigation_id = $2",
    )
    .bind(address_id)
    .bind(investigation_id)
    .fetch_optional(&state.database)
    .await?
    .ok_or(ApiError::NotFound)?;

    if address.network == "xrpl" {
        return crate::xrpl_sync::sync_tracked_address(state, address_id, &address.address).await;
    }

    if address.network == "solana" {
        return crate::solana_sync::sync_tracked_address(state, address_id, &address.address).await;
    }
    if address.network == "tron" {
        return crate::tron_sync::sync_tracked_address(state, address_id, &address.address).await;
    }
    if address.network == "ton" {
        return crate::ton_sync::sync_tracked_address(state, address_id, &address.address).await;
    }

    if address.network != "bitcoin" {
        let result = crate::evm_sync::sync_evm_address(
            state,
            address_id,
            &address.network,
            &address.address,
        )
        .await?;
        return Ok(result);
    }

    let transactions = fetch_history(state, &address.address).await?;
    let provider = provider_name(&state.bitcoin_esplora_url);
    let source_host = provider_host(&state.bitcoin_esplora_url);
    let observed_at = Utc::now();
    let mut saved = 0;
    for transaction in &transactions.items {
        let Some(transfer) = normalize_transfer(transaction, &address.address) else {
            continue;
        };
        let result = sqlx::query(
            "INSERT INTO observed_transfers (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, received_amount, spent_amount, asset, block_height, block_time, confirmed, provider, source_host, observed_at) \
             VALUES ($1, 'bitcoin', $2, -1, 'bitcoin', $3, $4, $5, 'BTC', $6, $7, $8, $9, $10, $11) \
             ON CONFLICT (tracked_address_id, transaction_hash, transfer_kind, event_index) DO UPDATE SET direction = EXCLUDED.direction, received_amount = EXCLUDED.received_amount, spent_amount = EXCLUDED.spent_amount, block_height = EXCLUDED.block_height, block_time = EXCLUDED.block_time, confirmed = EXCLUDED.confirmed, provider = EXCLUDED.provider, source_host = EXCLUDED.source_host, observed_at = EXCLUDED.observed_at",
        )
        .bind(address_id)
        .bind(transfer.transaction_hash)
        .bind(transfer.direction)
        .bind(transfer.received_amount)
        .bind(transfer.spent_amount)
        .bind(transfer.block_height)
        .bind(transfer.block_time)
        .bind(transfer.confirmed)
        .bind(&provider)
        .bind(&source_host)
        .bind(observed_at)
            .execute(&state.database)
        .await?;
        saved += result.rows_affected() as usize;
    }

    Ok(SyncResult {
        provider,
        fetched: transactions.items.len(),
        saved,
        pages: transactions.pages,
        may_have_more_history: transactions.may_have_more,
        observed_at,
    })
}

fn provider_name(base_url: &str) -> String {
    match reqwest::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
    {
        Some(host) => format!("Esplora ({host})"),
        None => "Esplora endpoint".into(),
    }
}

fn provider_host(base_url: &str) -> String {
    reqwest::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "endpoint".into())
}

pub(crate) async fn list_address_transfers(
    State(state): State<ApiState>,
    Path((investigation_id, address_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<ObservedTransfer>>, ApiError> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM tracked_addresses WHERE id = $1 AND investigation_id = $2)",
    )
    .bind(address_id)
    .bind(investigation_id)
    .fetch_one(&state.database)
    .await?;
    if !exists {
        return Err(ApiError::NotFound);
    }

    let transfers = sqlx::query_as::<_, ObservedTransfer>(
        "SELECT id, tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, received_amount::text AS received_amount, spent_amount::text AS spent_amount, asset, token_contract_address, token_decimals, from_address, to_address, block_height, block_time, confirmed, provider, source_host, observed_at FROM observed_transfers WHERE tracked_address_id = $1 ORDER BY block_time DESC NULLS LAST, observed_at DESC LIMIT 500",
    )
    .bind(address_id)
    .fetch_all(&state.database)
    .await?;
    Ok(Json(transfers))
}

struct History {
    items: Vec<EsploraTransaction>,
    pages: usize,
    may_have_more: bool,
}

async fn fetch_history(state: &ApiState, address: &str) -> Result<History, ApiError> {
    let url = format!(
        "{}/address/{}/txs",
        state.bitcoin_esplora_url.trim_end_matches('/'),
        address
    );
    let initial = state
        .http_client
        .get(url)
        .send()
        .await
        .map_err(|_| ApiError::ProviderUnavailable)?;
    if !initial.status().is_success() {
        return Err(ApiError::ProviderUnavailable);
    }
    let mut items: Vec<EsploraTransaction> = initial
        .json()
        .await
        .map_err(|_| ApiError::ProviderUnavailable)?;
    let mut confirmed: Vec<_> = items.iter().filter(|tx| tx.status.confirmed).collect();
    let mut pages = 1;
    while confirmed.len() >= 25 && pages < MAX_CONFIRMED_PAGES {
        let cursor = confirmed
            .last()
            .map(|tx| tx.txid.as_str())
            .ok_or(ApiError::ProviderUnavailable)?;
        let url = format!(
            "{}/address/{}/txs/chain/{}",
            state.bitcoin_esplora_url.trim_end_matches('/'),
            address,
            cursor
        );
        let response = state
            .http_client
            .get(url)
            .send()
            .await
            .map_err(|_| ApiError::ProviderUnavailable)?;
        if !response.status().is_success() {
            return Err(ApiError::ProviderUnavailable);
        }
        let next: Vec<EsploraTransaction> = response
            .json()
            .await
            .map_err(|_| ApiError::ProviderUnavailable)?;
        let count = next.len();
        items.extend(next);
        pages += 1;
        confirmed = items.iter().filter(|tx| tx.status.confirmed).collect();
        if count < 25 {
            break;
        }
    }
    let may_have_more = pages == MAX_CONFIRMED_PAGES && confirmed.len() >= pages * 25;
    Ok(History {
        items,
        pages,
        may_have_more,
    })
}

struct NormalizedTransfer {
    transaction_hash: String,
    direction: &'static str,
    received_amount: String,
    spent_amount: String,
    block_height: Option<i64>,
    block_time: Option<DateTime<Utc>>,
    confirmed: bool,
}

fn normalize_transfer(tx: &EsploraTransaction, address: &str) -> Option<NormalizedTransfer> {
    let received: u128 = tx
        .vout
        .iter()
        .filter(|output| output.scriptpubkey_address.as_deref() == Some(address))
        .map(|output| u128::from(output.value))
        .sum();
    let spent: u128 = tx
        .vin
        .iter()
        .filter_map(|input| input.prevout.as_ref())
        .filter(|output| output.scriptpubkey_address.as_deref() == Some(address))
        .map(|output| u128::from(output.value))
        .sum();
    if received == 0 && spent == 0 {
        return None;
    }
    let direction = match (received > 0, spent > 0) {
        (true, true) => "both",
        (true, false) => "inbound",
        (false, true) => "outbound",
        (false, false) => return None,
    };
    let block_time = tx
        .status
        .block_time
        .and_then(|timestamp| Utc.timestamp_opt(timestamp, 0).single());
    Some(NormalizedTransfer {
        transaction_hash: tx.txid.clone(),
        direction,
        received_amount: received.to_string(),
        spent_amount: spent.to_string(),
        block_height: tx.status.block_height,
        block_time,
        confirmed: tx.status.confirmed,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        EsploraInput, EsploraOutput, EsploraStatus, EsploraTransaction, normalize_transfer,
    };

    #[test]
    fn normalizes_address_level_input_output_and_change() {
        let address = "bc1qexample";
        let tx = EsploraTransaction {
            txid: "abc".into(),
            vin: vec![EsploraInput {
                prevout: Some(EsploraOutput {
                    scriptpubkey_address: Some(address.into()),
                    value: 1_000,
                }),
            }],
            vout: vec![
                EsploraOutput {
                    scriptpubkey_address: Some(address.into()),
                    value: 300,
                },
                EsploraOutput {
                    scriptpubkey_address: Some("bc1qother".into()),
                    value: 650,
                },
            ],
            status: EsploraStatus {
                confirmed: true,
                block_height: Some(10),
                block_time: Some(1_700_000_000),
            },
        };
        let normalized = normalize_transfer(&tx, address).unwrap();
        assert_eq!(normalized.direction, "both");
        assert_eq!(normalized.received_amount, "300");
        assert_eq!(normalized.spent_amount, "1000");
        assert!(normalized.confirmed);
    }

    #[test]
    fn ignores_transactions_without_address_activity() {
        let tx = EsploraTransaction {
            txid: "abc".into(),
            vin: vec![],
            vout: vec![],
            status: EsploraStatus::default(),
        };
        assert!(normalize_transfer(&tx, "bc1qexample").is_none());
    }

    #[test]
    fn records_the_configured_provider_host_as_source() {
        assert_eq!(
            super::provider_name("https://blockstream.info/api"),
            "Esplora (blockstream.info)"
        );
        assert_eq!(super::provider_name("not a URL"), "Esplora endpoint");
    }
}
