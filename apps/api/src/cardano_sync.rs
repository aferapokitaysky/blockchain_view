use std::{collections::BTreeMap, env, sync::Arc, time::Duration};

use axum::http::HeaderValue;
use chrono::{DateTime, Utc};
use redis::Client as RedisClient;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tokio::{sync::Mutex, time::Instant};
use tracing::warn;
use uuid::Uuid;

use crate::{ApiState, error::ApiError, transfers::SyncResult};

const API_BASE_URL: &str = "https://cardano-mainnet.blockfrost.io/api/v0/";
const PROVIDER: &str = "Blockfrost Cardano Mainnet";
const SOURCE_HOST: &str = "cardano-mainnet.blockfrost.io";
const PAGE_SIZE: u32 = 20;
const MAX_UTXO_ENTRIES: usize = 500;
const MAX_ASSETS_PER_ADDRESS: usize = 200;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const REQUEST_INTERVAL: Duration = Duration::from_millis(120);
const RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRIES: usize = 1;
const REDIS_GATE_KEY: &str = "tracefold:blockfrost:cardano:request-gate";

#[derive(Clone)]
pub struct CardanoClient {
    client: Client,
    base_url: Url,
    project_id: Option<HeaderValue>,
    redis: Option<RedisClient>,
    local_request_gate: Arc<Mutex<Option<Instant>>>,
}

#[derive(Debug, FromRow)]
struct SyncCursor {
    newest_tx_hash: Option<String>,
    newest_block_height: Option<i64>,
    newest_tx_index: Option<i32>,
    next_backfill_page: i32,
    backfill_exhausted: bool,
}

#[derive(Debug, Deserialize, Clone)]
struct AddressTransaction {
    tx_hash: String,
    block_height: i64,
    tx_index: i32,
    block_time: i64,
}

#[derive(Debug, Deserialize)]
struct AssetAmount {
    unit: String,
    quantity: String,
}

#[derive(Debug, Deserialize)]
struct TransactionUtxo {
    address: String,
    amount: Vec<AssetAmount>,
    #[serde(default)]
    collateral: bool,
    #[serde(default)]
    reference: bool,
}

#[derive(Debug, Deserialize)]
struct TransactionUtxos {
    hash: String,
    inputs: Vec<TransactionUtxo>,
    outputs: Vec<TransactionUtxo>,
}

#[derive(Debug, Deserialize)]
struct TransactionSummary {
    valid_contract: bool,
}

#[derive(Debug)]
struct CardanoTransfer {
    transaction_hash: String,
    event_index: i64,
    transfer_kind: &'static str,
    direction: &'static str,
    received_amount: String,
    spent_amount: String,
    asset: String,
    asset_id: Option<String>,
    token_decimals: Option<i16>,
    block_height: i64,
    block_time: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
struct CardanoRelationship {
    transaction_hash: String,
    counterparty_address: String,
    participant_role: &'static str,
    block_height: i64,
    block_time: DateTime<Utc>,
}

#[derive(Debug)]
struct ParsedTransaction {
    transfers: Vec<CardanoTransfer>,
    relationships: Vec<CardanoRelationship>,
}

impl CardanoClient {
    pub fn from_env(client: Client, redis: RedisClient) -> Result<Self, CardanoConfigError> {
        let project_id = env::var("BLOCKFROST_PROJECT_ID")
            .ok()
            .filter(|value| !value.trim().is_empty());
        Self::new(project_id.as_deref(), client, Some(redis))
    }

    pub(crate) fn new(
        project_id: Option<&str>,
        client: Client,
        redis: Option<RedisClient>,
    ) -> Result<Self, CardanoConfigError> {
        let project_id = project_id
            .map(|value| {
                HeaderValue::try_from(value).map_err(|_| CardanoConfigError::InvalidProjectId)
            })
            .transpose()?;
        Ok(Self {
            client,
            base_url: Url::parse(API_BASE_URL).expect("Blockfrost mainnet URL is valid"),
            project_id,
            redis,
            local_request_gate: Arc::new(Mutex::new(None)),
        })
    }

    pub fn is_configured(&self) -> bool {
        self.project_id.is_some()
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
                .arg(REDIS_GATE_KEY)
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

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, ApiError> {
        let project_id = self
            .project_id
            .as_ref()
            .ok_or(ApiError::ProviderUnavailable)?;
        let url = self
            .base_url
            .join(path)
            .map_err(|_| ApiError::ProviderUnavailable)?;
        for attempt in 0..=MAX_RETRIES {
            self.pace_request().await?;
            let response = self
                .client
                .get(url.clone())
                .header("project_id", project_id.clone())
                .query(query)
                .send()
                .await
                .map_err(|_| ApiError::ProviderUnavailable)?;
            if response.status().is_success() {
                if response
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
                return serde_json::from_slice(&body).map_err(|_| ApiError::ProviderUnavailable);
            }
            let status = response.status();
            if attempt < MAX_RETRIES
                && (status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error())
            {
                let retry_after = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .map(|seconds| Duration::from_secs(seconds.clamp(1, 5)))
                    .unwrap_or(RETRY_DELAY);
                tokio::time::sleep(retry_after).await;
                continue;
            }
            warn!(
                provider = SOURCE_HOST,
                status = status.as_u16(),
                "Blockfrost API request failed"
            );
            return Err(ApiError::ProviderUnavailable);
        }
        Err(ApiError::ProviderUnavailable)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CardanoConfigError {
    #[error("BLOCKFROST_PROJECT_ID contains invalid header characters")]
    InvalidProjectId,
}

async fn pace_local(gate: &Mutex<Option<Instant>>) {
    let mut last = gate.lock().await;
    if let Some(previous) = *last {
        tokio::time::sleep(REQUEST_INTERVAL.saturating_sub(previous.elapsed())).await;
    }
    *last = Some(Instant::now());
}

impl CardanoClient {
    async fn address_transactions(
        &self,
        address: &str,
        page: u32,
        order: &str,
        from: Option<String>,
    ) -> Result<Vec<AddressTransaction>, ApiError> {
        self.get_json(
            &format!("addresses/{address}/transactions"),
            &[
                ("count", PAGE_SIZE.to_string()),
                ("page", page.to_string()),
                ("order", order.to_owned()),
                ("from", from.unwrap_or_default()),
            ],
        )
        .await
    }

    async fn transaction_utxos(&self, hash: &str) -> Result<TransactionUtxos, ApiError> {
        self.get_json(&format!("txs/{hash}/utxos"), &[]).await
    }

    async fn transaction_summary(&self, hash: &str) -> Result<TransactionSummary, ApiError> {
        self.get_json(&format!("txs/{hash}"), &[]).await
    }
}

pub(crate) async fn sync_tracked_address(
    state: &ApiState,
    tracked_address_id: Uuid,
    address: &str,
) -> Result<SyncResult, ApiError> {
    if !state.cardano.is_configured() {
        return Err(ApiError::ProviderUnavailable);
    }

    let cursor = sqlx::query_as::<_, SyncCursor>(
        "SELECT newest_tx_hash, newest_block_height, newest_tx_index, next_backfill_page, backfill_exhausted FROM cardano_sync_cursors \
         WHERE tracked_address_id = $1",
    )
    .bind(tracked_address_id)
    .fetch_optional(&state.database)
    .await?;
    let (cursor_hash, cursor_height, cursor_index, next_page, backfill_exhausted) = cursor
        .map(|cursor| {
            (
                cursor.newest_tx_hash,
                cursor.newest_block_height,
                cursor.newest_tx_index,
                cursor.next_backfill_page,
                cursor.backfill_exhausted,
            )
        })
        .unwrap_or((None, None, None, 1, false));

    // `from` включителен, поэтому ниже отбрасывается сама контрольная транзакция.
    let head = if let (Some(height), Some(index)) = (cursor_height, cursor_index) {
        state
            .cardano
            .address_transactions(address, 1, "asc", Some(format!("{height}:{index}")))
            .await?
            .into_iter()
            .filter(|transaction| {
                (transaction.block_height, transaction.tx_index) > (height, index)
            })
            .take(PAGE_SIZE as usize)
            .collect::<Vec<_>>()
    } else {
        state
            .cardano
            .address_transactions(address, 1, "desc", None)
            .await?
    };
    let backfill = if backfill_exhausted {
        Vec::new()
    } else {
        state
            .cardano
            .address_transactions(address, next_page as u32, "asc", None)
            .await?
    };
    let mut unique_transactions = BTreeMap::<String, AddressTransaction>::new();
    for transaction in head.iter().chain(&backfill) {
        if !is_transaction_hash(&transaction.tx_hash) || transaction.block_height < 0 {
            return Err(ApiError::ProviderUnavailable);
        }
        unique_transactions
            .entry(transaction.tx_hash.to_ascii_lowercase())
            .or_insert_with(|| transaction.clone());
    }

    let mut parsed_transactions = Vec::with_capacity(unique_transactions.len());
    for transaction in unique_transactions.values() {
        let utxos = state
            .cardano
            .transaction_utxos(&transaction.tx_hash)
            .await?;
        if !utxos.hash.eq_ignore_ascii_case(&transaction.tx_hash)
            || utxos.inputs.len().saturating_add(utxos.outputs.len()) > MAX_UTXO_ENTRIES
        {
            return Err(ApiError::ProviderUnavailable);
        }
        let has_collateral = utxos.inputs.iter().any(|input| input.collateral)
            || utxos.outputs.iter().any(|output| output.collateral);
        let valid_contract = if has_collateral {
            Some(
                state
                    .cardano
                    .transaction_summary(&transaction.tx_hash)
                    .await?
                    .valid_contract,
            )
        } else {
            None
        };
        let block_time = DateTime::from_timestamp(transaction.block_time, 0)
            .ok_or(ApiError::ProviderUnavailable)?;
        parsed_transactions.push(parse_transaction(
            utxos,
            address,
            transaction.block_height,
            block_time,
            valid_contract,
        )?);
    }

    let observed_at = Utc::now();
    let mut database_transaction = state.database.begin().await?;
    let mut saved = 0;
    for parsed in &parsed_transactions {
        for transfer in &parsed.transfers {
            let result = sqlx::query(
                "INSERT INTO observed_transfers \
                 (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, \
                  received_amount, spent_amount, asset, token_contract_address, token_decimals, \
                  from_address, to_address, block_height, block_time, confirmed, provider, source_host, observed_at) \
                 VALUES ($1, 'cardano', $2, $3, $4, $5, $6, $7, $8, $9, $10, NULL, NULL, $11, $12, TRUE, $13, $14, $15) \
                 ON CONFLICT (tracked_address_id, transaction_hash, transfer_kind, event_index) \
                 DO UPDATE SET direction = EXCLUDED.direction, received_amount = EXCLUDED.received_amount, \
                  spent_amount = EXCLUDED.spent_amount, asset = EXCLUDED.asset, \
                  token_contract_address = EXCLUDED.token_contract_address, token_decimals = EXCLUDED.token_decimals, \
                  block_height = EXCLUDED.block_height, block_time = EXCLUDED.block_time, provider = EXCLUDED.provider, \
                  source_host = EXCLUDED.source_host, observed_at = EXCLUDED.observed_at",
            )
            .bind(tracked_address_id)
            .bind(&transfer.transaction_hash)
            .bind(transfer.event_index)
            .bind(transfer.transfer_kind)
            .bind(transfer.direction)
            .bind(&transfer.received_amount)
            .bind(&transfer.spent_amount)
            .bind(&transfer.asset)
            .bind(&transfer.asset_id)
            .bind(transfer.token_decimals)
            .bind(transfer.block_height)
            .bind(transfer.block_time)
            .bind(PROVIDER)
            .bind(SOURCE_HOST)
            .bind(observed_at)
            .execute(&mut *database_transaction)
            .await?;
            saved += result.rows_affected() as usize;
        }
        for relationship in &parsed.relationships {
            let result = sqlx::query(
                "INSERT INTO observed_relationships \
                 (tracked_address_id, network, transaction_hash, counterparty_address, relationship_type, \
                  participant_role, block_height, block_time, confirmed, provider, source_host, observed_at) \
                 VALUES ($1, 'cardano', $2, $3, 'same_transaction', $4, $5, $6, TRUE, $7, $8, $9) \
                 ON CONFLICT (tracked_address_id, transaction_hash, counterparty_address, relationship_type) \
                 DO UPDATE SET participant_role = EXCLUDED.participant_role, block_height = EXCLUDED.block_height, \
                  block_time = EXCLUDED.block_time, provider = EXCLUDED.provider, source_host = EXCLUDED.source_host, \
                  observed_at = EXCLUDED.observed_at",
            )
            .bind(tracked_address_id)
            .bind(&relationship.transaction_hash)
            .bind(&relationship.counterparty_address)
            .bind(relationship.participant_role)
            .bind(relationship.block_height)
            .bind(relationship.block_time)
            .bind(PROVIDER)
            .bind(SOURCE_HOST)
            .bind(observed_at)
            .execute(&mut *database_transaction)
            .await?;
            saved += result.rows_affected() as usize;
        }
    }

    let newest = head
        .iter()
        .max_by_key(|transaction| (transaction.block_height, transaction.tx_index));
    let newest_tx_hash = newest
        .map(|transaction| transaction.tx_hash.to_ascii_lowercase())
        .or(cursor_hash);
    let newest_block_height = newest
        .map(|transaction| transaction.block_height)
        .or(cursor_height);
    let newest_tx_index = newest
        .map(|transaction| transaction.tx_index)
        .or(cursor_index);
    let backfill_page_full = backfill.len() == PAGE_SIZE as usize;
    let updated_page = if backfill_page_full {
        next_page.saturating_add(1)
    } else {
        next_page
    };
    let is_backfill_exhausted = backfill_exhausted || !backfill_page_full;
    sqlx::query(
        "INSERT INTO cardano_sync_cursors (tracked_address_id, newest_tx_hash, newest_block_height, newest_tx_index, next_backfill_page, backfill_exhausted) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (tracked_address_id) DO UPDATE SET \
          newest_tx_hash = COALESCE(EXCLUDED.newest_tx_hash, cardano_sync_cursors.newest_tx_hash), \
          newest_block_height = COALESCE(EXCLUDED.newest_block_height, cardano_sync_cursors.newest_block_height), \
          newest_tx_index = COALESCE(EXCLUDED.newest_tx_index, cardano_sync_cursors.newest_tx_index), \
          next_backfill_page = GREATEST(cardano_sync_cursors.next_backfill_page, EXCLUDED.next_backfill_page), \
          backfill_exhausted = cardano_sync_cursors.backfill_exhausted OR EXCLUDED.backfill_exhausted, \
          updated_at = now()",
    )
    .bind(tracked_address_id)
    .bind(newest_tx_hash)
    .bind(newest_block_height)
    .bind(newest_tx_index)
    .bind(updated_page)
    .bind(is_backfill_exhausted)
    .execute(&mut *database_transaction)
    .await?;
    database_transaction.commit().await?;

    Ok(SyncResult {
        provider: format!("{PROVIDER} ({SOURCE_HOST})"),
        fetched: unique_transactions.len(),
        saved,
        pages: 1 + usize::from(!backfill_exhausted),
        may_have_more_history: head.len() == PAGE_SIZE as usize || backfill_page_full,
        observed_at,
    })
}

fn parse_transaction(
    transaction: TransactionUtxos,
    tracked_address: &str,
    block_height: i64,
    block_time: DateTime<Utc>,
    valid_contract: Option<bool>,
) -> Result<ParsedTransaction, ApiError> {
    let tracked_address = crate::addresses::normalize_cardano_address(tracked_address)?;
    let has_collateral = transaction.inputs.iter().any(|input| input.collateral)
        || transaction.outputs.iter().any(|output| output.collateral);
    if has_collateral && valid_contract.is_none() {
        return Err(ApiError::ProviderUnavailable);
    }
    let is_valid = valid_contract.unwrap_or(true);
    let inputs = transaction.inputs.iter().filter(|input| {
        !input.reference
            && if is_valid {
                !input.collateral
            } else {
                input.collateral
            }
    });
    let outputs = transaction.outputs.iter().filter(|output| {
        if is_valid {
            !output.collateral
        } else {
            output.collateral
        }
    });
    let mut asset_totals = BTreeMap::<String, (String, String)>::new();
    let mut participants = BTreeMap::<String, (bool, bool)>::new();
    for input in inputs {
        let address = crate::addresses::normalize_cardano_address(&input.address)?;
        let participant = participants.entry(address.clone()).or_default();
        participant.0 = true;
        add_utxo_amounts(
            &mut asset_totals,
            &input.amount,
            &address,
            &tracked_address,
            true,
        )?;
    }
    for output in outputs {
        let address = crate::addresses::normalize_cardano_address(&output.address)?;
        let participant = participants.entry(address.clone()).or_default();
        participant.1 = true;
        add_utxo_amounts(
            &mut asset_totals,
            &output.amount,
            &address,
            &tracked_address,
            false,
        )?;
    }

    let mut transfers = Vec::new();
    for (event_index, (unit, (spent, received))) in asset_totals.into_iter().enumerate() {
        if spent == "0" && received == "0" {
            continue;
        }
        let (transfer_kind, asset, asset_id, decimals) = if unit == "lovelace" {
            ("cardano_native", "ADA".to_owned(), None, Some(6))
        } else {
            (
                "cardano_asset",
                cardano_asset_label(&unit),
                Some(unit),
                None,
            )
        };
        let direction = match (spent != "0", received != "0") {
            (true, true) => "both",
            (true, false) => "outbound",
            (false, true) => "inbound",
            (false, false) => continue,
        };
        transfers.push(CardanoTransfer {
            transaction_hash: transaction.hash.to_ascii_lowercase(),
            event_index: i64::try_from(event_index).map_err(|_| ApiError::ProviderUnavailable)?,
            transfer_kind,
            direction,
            received_amount: received,
            spent_amount: spent,
            asset,
            asset_id,
            token_decimals: decimals,
            block_height,
            block_time,
        });
    }

    let relationships = participants
        .into_iter()
        .filter_map(|(address, (is_input, is_output))| {
            if address == tracked_address {
                return None;
            }
            let participant_role = match (is_input, is_output) {
                (true, true) => "input_and_output",
                (true, false) => "input",
                (false, true) => "output",
                (false, false) => return None,
            };
            Some(CardanoRelationship {
                transaction_hash: transaction.hash.to_ascii_lowercase(),
                counterparty_address: address,
                participant_role,
                block_height,
                block_time,
            })
        })
        .collect();

    Ok(ParsedTransaction {
        transfers,
        relationships,
    })
}

fn add_utxo_amounts(
    totals: &mut BTreeMap<String, (String, String)>,
    amounts: &[AssetAmount],
    address: &str,
    tracked_address: &str,
    is_input: bool,
) -> Result<(), ApiError> {
    if amounts.len() > MAX_ASSETS_PER_ADDRESS {
        return Err(ApiError::ProviderUnavailable);
    }
    if address != tracked_address {
        return Ok(());
    }
    for amount in amounts {
        let unit = amount.unit.to_ascii_lowercase();
        if !is_valid_asset_unit(&unit) {
            return Err(ApiError::ProviderUnavailable);
        }
        let quantity = parse_raw_quantity(&amount.quantity)?;
        let totals = totals
            .entry(unit)
            .or_insert_with(|| ("0".into(), "0".into()));
        let target = if is_input {
            &mut totals.0
        } else {
            &mut totals.1
        };
        *target = add_raw_quantities(target, &quantity)?;
    }
    Ok(())
}

fn parse_raw_quantity(value: &str) -> Result<String, ApiError> {
    if value.is_empty() || value.len() > 78 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ApiError::ProviderUnavailable);
    }
    Ok(value.to_owned())
}

fn add_raw_quantities(left: &str, right: &str) -> Result<String, ApiError> {
    let mut carry = 0u8;
    let mut result = Vec::with_capacity(left.len().max(right.len()) + 1);
    let mut left = left.bytes().rev();
    let mut right = right.bytes().rev();
    loop {
        let left_digit = left.next().map(|value| value - b'0');
        let right_digit = right.next().map(|value| value - b'0');
        if left_digit.is_none() && right_digit.is_none() && carry == 0 {
            break;
        }
        let sum = left_digit.unwrap_or(0) + right_digit.unwrap_or(0) + carry;
        result.push(b'0' + (sum % 10));
        carry = sum / 10;
    }
    if result.len() > 78 {
        return Err(ApiError::ProviderUnavailable);
    }
    result.reverse();
    String::from_utf8(result).map_err(|_| ApiError::ProviderUnavailable)
}

fn is_valid_asset_unit(unit: &str) -> bool {
    unit == "lovelace"
        || ((56..=120).contains(&unit.len())
            && unit.len().is_multiple_of(2)
            && unit.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn cardano_asset_label(unit: &str) -> String {
    let policy_id = &unit[..56];
    let asset_name = hex::decode(&unit[56..])
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|name| {
            !name.is_empty()
                && name.chars().count() <= 32
                && name.chars().all(|character| character.is_ascii_graphic())
        });
    asset_name.unwrap_or_else(|| format!("Asset {}…", &policy_id[..8]))
}

fn is_transaction_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACKED: &str = "addr1qx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3n0d3vllmyqwsx5wktcd8cc3sq835lu7drv2xwl2wywfgse35a3x";
    const OTHER: &str = "Ae2tdPwUPEZLs4HtbuNey7tK4hTKrwNwYtGqp7bDfCy2WdR3P6735W5Yfpe";

    fn utxo(address: &str, quantity: &str, collateral: bool, reference: bool) -> TransactionUtxo {
        TransactionUtxo {
            address: address.to_owned(),
            amount: vec![AssetAmount {
                unit: "lovelace".to_owned(),
                quantity: quantity.to_owned(),
            }],
            collateral,
            reference,
        }
    }

    fn transaction(
        inputs: Vec<TransactionUtxo>,
        outputs: Vec<TransactionUtxo>,
    ) -> TransactionUtxos {
        TransactionUtxos {
            hash: "a".repeat(64),
            inputs,
            outputs,
        }
    }

    #[test]
    fn parses_tracked_utxo_deltas_without_inventing_direct_transfers() {
        let parsed = parse_transaction(
            transaction(
                vec![
                    utxo(TRACKED, "1000000", false, false),
                    utxo(OTHER, "500000", false, false),
                ],
                vec![
                    utxo(TRACKED, "250000", false, false),
                    utxo(OTHER, "1200000", false, false),
                ],
            ),
            TRACKED,
            42,
            DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            None,
        )
        .unwrap();

        assert_eq!(parsed.transfers.len(), 1);
        assert_eq!(parsed.transfers[0].transfer_kind, "cardano_native");
        assert_eq!(parsed.transfers[0].spent_amount, "1000000");
        assert_eq!(parsed.transfers[0].received_amount, "250000");
        assert_eq!(parsed.transfers[0].direction, "both");
        assert_eq!(parsed.relationships.len(), 1);
        assert_eq!(parsed.relationships[0].counterparty_address, OTHER);
        assert_eq!(parsed.relationships[0].participant_role, "input_and_output");
    }

    #[test]
    fn ignores_reference_inputs_and_uses_collateral_for_failed_scripts() {
        let valid = parse_transaction(
            transaction(
                vec![
                    utxo(TRACKED, "900", false, true),
                    utxo(TRACKED, "100", false, false),
                ],
                vec![utxo(TRACKED, "75", false, false)],
            ),
            TRACKED,
            42,
            DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(valid.transfers[0].spent_amount, "100");

        let failed = parse_transaction(
            transaction(
                vec![
                    utxo(TRACKED, "900", false, false),
                    utxo(TRACKED, "100", true, false),
                ],
                vec![
                    utxo(TRACKED, "75", false, false),
                    utxo(TRACKED, "60", true, false),
                ],
            ),
            TRACKED,
            42,
            DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            Some(false),
        )
        .unwrap();
        assert_eq!(failed.transfers[0].spent_amount, "100");
        assert_eq!(failed.transfers[0].received_amount, "60");
    }

    #[test]
    fn keeps_native_asset_quantities_exact_and_rejects_bad_values() {
        assert_eq!(
            add_raw_quantities("99999999999999999999", "1").unwrap(),
            "100000000000000000000"
        );
        assert!(parse_raw_quantity("-1").is_err());
        assert!(parse_raw_quantity(&"9".repeat(79)).is_err());
        assert!(is_valid_asset_unit(&format!(
            "{}{}",
            "ab".repeat(28),
            "cd".repeat(4)
        )));
        assert!(!is_valid_asset_unit("ab".repeat(27).as_str()));
    }
}
