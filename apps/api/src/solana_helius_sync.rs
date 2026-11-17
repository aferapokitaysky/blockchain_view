use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{ApiState, error::ApiError, transfers::SyncResult};

const ENDPOINT: &str = "https://mainnet.helius-rpc.com/";
const PAGE_SIZE: usize = 100;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct RpcEnvelope {
    result: Option<Page>,
    error: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    #[serde(default)]
    data: Vec<TransferEvent>,
    pagination_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransferEvent {
    signature: String,
    slot: u64,
    block_time: Option<i64>,
    from_user_account: Option<String>,
    to_user_account: Option<String>,
    mint: Option<String>,
    amount: String,
    decimals: u8,
    instruction_idx: Option<u16>,
    inner_instruction_idx: Option<u16>,
    confirmation_status: Option<String>,
}

#[derive(FromRow)]
struct Cursor {
    head_slot: i64,
    backfill_token: Option<String>,
    backfill_exhausted: bool,
}

#[derive(Clone)]
struct Observed {
    signature: String,
    event_index: i64,
    kind: &'static str,
    direction: &'static str,
    received: String,
    spent: String,
    asset: String,
    mint: Option<String>,
    decimals: i16,
    from: Option<String>,
    to: Option<String>,
    slot: i64,
    block_time: Option<DateTime<Utc>>,
}

pub(crate) async fn sync_tracked_address(
    state: &ApiState,
    address_id: Uuid,
    address: &str,
) -> Result<SyncResult, ApiError> {
    let key = state
        .solana_helius_api_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
        .ok_or(ApiError::ProviderUnavailable)?;
    let cursor = sqlx::query_as::<_, Cursor>(
        "SELECT head_slot, backfill_token, backfill_exhausted FROM solana_helius_sync_cursors WHERE tracked_address_id = $1",
    )
    .bind(address_id)
    .fetch_optional(&state.database)
    .await?;

    let mut head_slot = cursor.as_ref().map_or(0, |cursor| cursor.head_slot);
    let mut backfill_token = cursor
        .as_ref()
        .and_then(|cursor| cursor.backfill_token.clone());
    let mut backfill_exhausted = cursor
        .as_ref()
        .is_some_and(|cursor| cursor.backfill_exhausted);

    let head = request_page(
        state,
        key,
        address,
        None,
        (head_slot > 0).then_some(head_slot),
    )
    .await?;
    head_slot = head_slot.max(head.data.iter().map(|event| event.slot).max().unwrap_or(0) as i64);
    if cursor.is_none() {
        backfill_token = head.pagination_token.clone();
        backfill_exhausted = head.pagination_token.is_none();
    }
    let mut events = head.data;
    let mut pages = 1;
    if !backfill_exhausted {
        if let Some(token) = backfill_token.as_deref() {
            let older = request_page(state, key, address, Some(token), None).await?;
            (backfill_token, backfill_exhausted) = advance_cursor(token, older.pagination_token);
            events.extend(older.data);
            pages += 1;
        } else {
            backfill_exhausted = true;
        }
    }

    let observed: Vec<_> = events
        .into_iter()
        .filter_map(|event| parse_event(event, address))
        .collect();
    let now = Utc::now();
    let mut tx = state.database.begin().await?;
    let mut saved = 0usize;
    for event in &observed {
        let result = sqlx::query(
            "INSERT INTO observed_transfers (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, received_amount, spent_amount, asset, token_contract_address, token_decimals, from_address, to_address, block_height, block_time, confirmed, provider, source_host, observed_at) VALUES ($1, 'solana', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, TRUE, 'Helius getTransfersByAddress', 'mainnet.helius-rpc.com', $15) ON CONFLICT (tracked_address_id, transaction_hash, transfer_kind, event_index) DO UPDATE SET direction = EXCLUDED.direction, received_amount = EXCLUDED.received_amount, spent_amount = EXCLUDED.spent_amount, asset = EXCLUDED.asset, token_contract_address = EXCLUDED.token_contract_address, token_decimals = EXCLUDED.token_decimals, from_address = EXCLUDED.from_address, to_address = EXCLUDED.to_address, block_height = EXCLUDED.block_height, block_time = EXCLUDED.block_time, provider = EXCLUDED.provider, source_host = EXCLUDED.source_host, observed_at = EXCLUDED.observed_at",
        )
        .bind(address_id).bind(&event.signature).bind(event.event_index).bind(event.kind)
        .bind(event.direction).bind(&event.received).bind(&event.spent).bind(&event.asset)
        .bind(&event.mint).bind(event.decimals).bind(&event.from).bind(&event.to)
        .bind(event.slot).bind(event.block_time).bind(now).execute(&mut *tx).await?;
        saved += result.rows_affected() as usize;
    }
    sqlx::query("INSERT INTO solana_helius_sync_cursors (tracked_address_id, head_slot, backfill_token, backfill_exhausted, updated_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (tracked_address_id) DO UPDATE SET head_slot = EXCLUDED.head_slot, backfill_token = EXCLUDED.backfill_token, backfill_exhausted = EXCLUDED.backfill_exhausted, updated_at = EXCLUDED.updated_at")
        .bind(address_id).bind(head_slot).bind(backfill_token).bind(backfill_exhausted).bind(now).execute(&mut *tx).await?;
    tx.commit().await?;

    Ok(SyncResult {
        provider: "Helius getTransfersByAddress (finalized)".into(),
        fetched: observed.len(),
        saved,
        pages,
        may_have_more_history: !backfill_exhausted,
        observed_at: now,
    })
}

fn advance_cursor(previous: &str, next: Option<String>) -> (Option<String>, bool) {
    let exhausted = next.as_deref().is_none_or(|token| token == previous);
    (next, exhausted)
}

async fn request_page(
    state: &ApiState,
    key: &str,
    address: &str,
    pagination_token: Option<&str>,
    slot_gte: Option<i64>,
) -> Result<Page, ApiError> {
    let mut filter = serde_json::Map::new();
    if let Some(slot) = slot_gte {
        filter.insert("slot".into(), json!({"gte": slot}));
    }
    let url = reqwest::Url::parse_with_params(ENDPOINT, [("api-key", key)])
        .map_err(|_| ApiError::ProviderUnavailable)?;
    let mut options = json!({
        "limit": PAGE_SIZE,
        "sortOrder": "desc",
        "commitment": "finalized",
        "solMode": "separate",
        "filters": filter
    });
    if let Some(pagination_token) = pagination_token {
        options["paginationToken"] = json!(pagination_token);
    }
    let response = state
        .http_client
        .post(url)
        .json(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "getTransfersByAddress",
            "params": [address, options],
        }))
        .send()
        .await
        .map_err(|_| ApiError::ProviderUnavailable)?;
    if !response.status().is_success() {
        return Err(ApiError::ProviderUnavailable);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(ApiError::ProviderUnavailable);
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ApiError::ProviderUnavailable)?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(ApiError::ProviderUnavailable);
        }
        bytes.extend_from_slice(&chunk);
    }
    let envelope: RpcEnvelope =
        serde_json::from_slice(&bytes).map_err(|_| ApiError::ProviderUnavailable)?;
    if envelope.error.is_some() {
        return Err(ApiError::ProviderUnavailable);
    }
    envelope.result.ok_or(ApiError::ProviderUnavailable)
}

fn parse_event(event: TransferEvent, tracked: &str) -> Option<Observed> {
    if event
        .confirmation_status
        .as_deref()
        .is_some_and(|status| status != "finalized")
    {
        return None;
    }
    let amount = event.amount.parse::<u128>().ok()?;
    if amount == 0 || event.signature.is_empty() || event.decimals > 38 {
        return None;
    }
    let (kind, asset, mint) = match event.mint.as_deref() {
        None | Some("So11111111111111111111111111111111111111111") => {
            ("solana_native", "SOL".to_owned(), None)
        }
        Some(mint) if valid_public_key(mint) => {
            ("solana_spl", mint.to_owned(), Some(mint.to_owned()))
        }
        _ => return None,
    };
    let direction = if event.to_user_account.as_deref() == Some(tracked) {
        "inbound"
    } else if event.from_user_account.as_deref() == Some(tracked) {
        "outbound"
    } else {
        return None;
    };
    let instruction = i64::from(event.instruction_idx?);
    let inner = i64::from(event.inner_instruction_idx?);
    let index = instruction.checked_mul(65_536)?.checked_add(inner)?;
    Some(Observed {
        signature: event.signature,
        event_index: index,
        kind,
        direction,
        received: if direction == "inbound" {
            amount.to_string()
        } else {
            "0".into()
        },
        spent: if direction == "outbound" {
            amount.to_string()
        } else {
            "0".into()
        },
        asset,
        mint,
        decimals: i16::from(event.decimals),
        from: event.from_user_account,
        to: event.to_user_account,
        slot: i64::try_from(event.slot).ok()?,
        block_time: event
            .block_time
            .and_then(|time| DateTime::from_timestamp(time, 0)),
    })
}

fn valid_public_key(value: &str) -> bool {
    (32..=44).contains(&value.len())
        && value.bytes().all(|byte| {
            b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz".contains(&byte)
        })
}

#[cfg(test)]
mod tests {
    use super::{RpcEnvelope, TransferEvent, parse_event};

    fn event() -> TransferEvent {
        serde_json::from_value(serde_json::json!({
            "signature":"sig", "slot":42, "blockTime":1700000000,
            "fromUserAccount":"11111111111111111111111111111111",
            "toUserAccount":"22222222222222222222222222222222",
            "mint":"TokenMint1111111111111111111111111111111111", "amount":"9007199254740993",
            "decimals":6, "transactionIdx":1, "instructionIdx":2, "innerInstructionIdx":3
        }))
        .unwrap()
    }

    #[test]
    fn retains_raw_precision_and_stable_instruction_identity() {
        let parsed = parse_event(event(), "22222222222222222222222222222222").unwrap();
        assert_eq!(parsed.received, "9007199254740993");
        assert_eq!(parsed.event_index, 131_075);
        assert_eq!(parsed.kind, "solana_spl");
    }

    #[test]
    fn ignores_events_unrelated_to_tracked_wallet() {
        assert!(parse_event(event(), "33333333333333333333333333333333").is_none());
    }

    #[test]
    fn reads_provider_cursor_for_the_next_backfill_page() {
        let response: RpcEnvelope = serde_json::from_value(serde_json::json!({
            "result": {"data": [], "paginationToken": "315073428:35:1:0:splTransfer"}
        }))
        .unwrap();
        assert_eq!(
            response.result.unwrap().pagination_token.as_deref(),
            Some("315073428:35:1:0:splTransfer")
        );
    }

    #[test]
    fn cursor_advances_or_stops_on_end_and_repeated_tokens() {
        assert_eq!(
            super::advance_cursor("cursor-a", Some("cursor-b".into())),
            (Some("cursor-b".into()), false)
        );
        assert_eq!(super::advance_cursor("cursor-a", None), (None, true));
        assert_eq!(
            super::advance_cursor("cursor-a", Some("cursor-a".into())),
            (Some("cursor-a".into()), true)
        );
    }
}
