use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{ApiState, error::ApiError, transfers::SyncResult};

// Fifteen sequential transaction RPC calls fit within the frontend sync timeout.
const SIGNATURE_PAGE_SIZE: usize = 15;
const MAX_TRANSACTION_VERSION: u8 = 1;

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignatureInfo {
    signature: String,
    #[serde(default)]
    err: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SolanaTransaction {
    slot: u64,
    block_time: Option<i64>,
    transaction: TransactionData,
    meta: Option<TransactionMeta>,
}

#[derive(Debug, Deserialize)]
struct TransactionData {
    message: TransactionMessage,
}

#[derive(Debug, Deserialize)]
struct TransactionMessage {
    #[serde(default, rename = "accountKeys")]
    account_keys: Vec<Value>,
    #[serde(default)]
    instructions: Vec<ParsedInstruction>,
}

#[derive(Debug, Deserialize)]
struct ParsedInstruction {
    program: Option<String>,
    parsed: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransactionMeta {
    err: Option<Value>,
    #[serde(default)]
    inner_instructions: Vec<InnerInstructionGroup>,
    #[serde(default, rename = "preTokenBalances")]
    pre_token_balances: Vec<TokenBalance>,
    #[serde(default, rename = "postTokenBalances")]
    post_token_balances: Vec<TokenBalance>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenBalance {
    account_index: usize,
    mint: String,
    owner: Option<String>,
    ui_token_amount: UiTokenAmount,
}

#[derive(Debug, Deserialize)]
struct UiTokenAmount {
    decimals: u8,
}

#[derive(Debug, Deserialize)]
struct InnerInstructionGroup {
    #[serde(default)]
    instructions: Vec<ParsedInstruction>,
}

#[derive(Debug, FromRow)]
struct SyncCursor {
    newest_scanned_signature: Option<String>,
    newest_pending_before_signature: Option<String>,
    newest_pending_head_signature: Option<String>,
    oldest_scanned_signature: Option<String>,
    backfill_exhausted: bool,
}

#[derive(Debug)]
struct NativeTransfer {
    event_index: i64,
    direction: &'static str,
    received_amount: u64,
    spent_amount: u64,
    transfer_kind: &'static str,
    asset: String,
    token_contract_address: Option<String>,
    token_decimals: Option<i16>,
    from_address: String,
    to_address: String,
}

#[derive(Debug)]
struct ObservedNativeTransfer {
    signature: String,
    block_height: Option<i64>,
    block_time: Option<DateTime<Utc>>,
    transfer: NativeTransfer,
}

pub(crate) async fn sync_tracked_address(
    state: &ApiState,
    address_id: Uuid,
    address: &str,
) -> Result<SyncResult, ApiError> {
    let cursor = sqlx::query_as::<_, SyncCursor>(
        "SELECT newest_scanned_signature, newest_pending_before_signature, \
         newest_pending_head_signature, oldest_scanned_signature, backfill_exhausted \
         FROM solana_sync_cursors WHERE tracked_address_id = $1",
    )
    .bind(address_id)
    .fetch_optional(&state.database)
    .await?;

    let mut next_newest_cursor = cursor
        .as_ref()
        .and_then(|cursor| cursor.newest_scanned_signature.clone());
    let mut next_pending_before = cursor
        .as_ref()
        .and_then(|cursor| cursor.newest_pending_before_signature.clone());
    let mut next_pending_head = cursor
        .as_ref()
        .and_then(|cursor| cursor.newest_pending_head_signature.clone());
    let mut next_oldest_cursor = cursor
        .as_ref()
        .and_then(|cursor| cursor.oldest_scanned_signature.clone());
    let mut backfill_exhausted = cursor
        .as_ref()
        .is_some_and(|cursor| cursor.backfill_exhausted);
    let signatures_to_fetch = if cursor.is_none() {
        let page = fetch_signatures(state, address, None, None).await?;
        next_newest_cursor = page.first().map(|item| item.signature.clone());
        next_oldest_cursor = page.last().map(|item| item.signature.clone());
        backfill_exhausted = page.len() < SIGNATURE_PAGE_SIZE;
        page
    } else if let (Some(before), Some(head)) =
        (next_pending_before.as_deref(), next_pending_head.as_deref())
    {
        let page =
            fetch_signatures(state, address, Some(before), next_newest_cursor.as_deref()).await?;
        if let Some(last) = page.last() {
            next_pending_before = Some(last.signature.clone());
        }
        if page.len() < SIGNATURE_PAGE_SIZE {
            next_newest_cursor = Some(head.to_owned());
            next_pending_before = None;
            next_pending_head = None;
        }
        page
    } else {
        let page = fetch_signatures(state, address, None, next_newest_cursor.as_deref()).await?;
        if page.len() == SIGNATURE_PAGE_SIZE {
            next_pending_head = page.first().map(|item| item.signature.clone());
            next_pending_before = page.last().map(|item| item.signature.clone());
        } else if let Some(first) = page.first() {
            next_newest_cursor = Some(first.signature.clone());
        }
        let pending = !page.is_empty();
        if pending || backfill_exhausted {
            page
        } else if let Some(before) = next_oldest_cursor.as_deref() {
            let older = fetch_signatures(state, address, Some(before), None).await?;
            if let Some(last) = older.last() {
                next_oldest_cursor = Some(last.signature.clone());
            }
            backfill_exhausted = older.len() < SIGNATURE_PAGE_SIZE;
            older
        } else {
            backfill_exhausted = true;
            page
        }
    };
    let pages = usize::from(!signatures_to_fetch.is_empty());

    let provider = provider_name(&state.solana_rpc_url);
    let source_host = provider_host(&state.solana_rpc_url);
    let mut native_transfers = Vec::new();
    for signature in &signatures_to_fetch {
        if signature.err.is_some() {
            continue;
        }
        let transaction = fetch_transaction(state, &signature.signature).await?;
        let Some(meta) = transaction.meta.as_ref() else {
            continue;
        };
        if meta.err.is_some() {
            continue;
        }
        native_transfers.extend(parse_supported_transfers(
            &transaction,
            &signature.signature,
            address,
        ));
    }

    let observed_at = Utc::now();
    let mut db_transaction = state.database.begin().await?;
    let mut saved = 0;
    for observed in &native_transfers {
        let result = sqlx::query(
            "INSERT INTO observed_transfers \
             (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, \
              received_amount, spent_amount, asset, token_contract_address, token_decimals, \
              from_address, to_address, block_height, block_time, \
              confirmed, provider, source_host, observed_at) \
             VALUES ($1, 'solana', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, TRUE, $15, $16, $17) \
             ON CONFLICT (tracked_address_id, transaction_hash, transfer_kind, event_index) \
             DO UPDATE SET direction = EXCLUDED.direction, received_amount = EXCLUDED.received_amount, \
              spent_amount = EXCLUDED.spent_amount, from_address = EXCLUDED.from_address, \
              token_contract_address = EXCLUDED.token_contract_address, token_decimals = EXCLUDED.token_decimals, \
              to_address = EXCLUDED.to_address, block_height = EXCLUDED.block_height, \
              block_time = EXCLUDED.block_time, confirmed = EXCLUDED.confirmed, provider = EXCLUDED.provider, \
              source_host = EXCLUDED.source_host, observed_at = EXCLUDED.observed_at",
        )
        .bind(address_id)
        .bind(&observed.signature)
        .bind(observed.transfer.event_index)
        .bind(observed.transfer.transfer_kind)
        .bind(observed.transfer.direction)
        .bind(observed.transfer.received_amount.to_string())
        .bind(observed.transfer.spent_amount.to_string())
        .bind(&observed.transfer.asset)
        .bind(&observed.transfer.token_contract_address)
        .bind(observed.transfer.token_decimals)
        .bind(&observed.transfer.from_address)
        .bind(&observed.transfer.to_address)
        .bind(observed.block_height)
        .bind(observed.block_time)
        .bind(&provider)
        .bind(&source_host)
        .bind(observed_at)
        .execute(&mut *db_transaction)
        .await?;
        saved += result.rows_affected() as usize;
    }

    let pending_history = next_pending_head.is_some() || !backfill_exhausted;
    sqlx::query(
        "INSERT INTO solana_sync_cursors \
         (tracked_address_id, newest_scanned_signature, newest_pending_before_signature, \
          newest_pending_head_signature, oldest_scanned_signature, backfill_exhausted, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (tracked_address_id) DO UPDATE SET \
          newest_scanned_signature = EXCLUDED.newest_scanned_signature, \
          newest_pending_before_signature = EXCLUDED.newest_pending_before_signature, \
          newest_pending_head_signature = EXCLUDED.newest_pending_head_signature, \
          oldest_scanned_signature = EXCLUDED.oldest_scanned_signature, \
          backfill_exhausted = EXCLUDED.backfill_exhausted, updated_at = EXCLUDED.updated_at",
    )
    .bind(address_id)
    .bind(next_newest_cursor)
    .bind(next_pending_before)
    .bind(next_pending_head)
    .bind(next_oldest_cursor)
    .bind(backfill_exhausted)
    .bind(observed_at)
    .execute(&mut *db_transaction)
    .await?;
    db_transaction.commit().await?;

    let fetched = signatures_to_fetch.len();
    Ok(SyncResult {
        provider,
        fetched,
        saved,
        pages,
        may_have_more_history: pending_history,
        observed_at,
    })
}

pub(crate) fn rpc_is_configured(endpoint: &str) -> bool {
    reqwest::Url::parse(endpoint).is_ok_and(|url| valid_rpc_url(&url))
}

async fn fetch_signatures(
    state: &ApiState,
    address: &str,
    before: Option<&str>,
    until: Option<&str>,
) -> Result<Vec<SignatureInfo>, ApiError> {
    let mut config = json!({
        "commitment": "finalized",
        "limit": SIGNATURE_PAGE_SIZE,
    });
    if let Some(before) = before {
        config["before"] = json!(before);
    }
    if let Some(until) = until {
        config["until"] = json!(until);
    }
    rpc_call(state, "getSignaturesForAddress", json!([address, config])).await
}

async fn fetch_transaction(
    state: &ApiState,
    signature: &str,
) -> Result<SolanaTransaction, ApiError> {
    rpc_call::<Option<SolanaTransaction>>(
        state,
        "getTransaction",
        json!([
            signature,
            {
                "commitment": "finalized",
                "encoding": "jsonParsed",
                "maxSupportedTransactionVersion": MAX_TRANSACTION_VERSION,
            }
        ]),
    )
    .await?
    .ok_or(ApiError::ProviderUnavailable)
}

async fn rpc_call<T: DeserializeOwned>(
    state: &ApiState,
    method: &str,
    params: Value,
) -> Result<T, ApiError> {
    let url =
        reqwest::Url::parse(&state.solana_rpc_url).map_err(|_| ApiError::ProviderUnavailable)?;
    if !valid_rpc_url(&url) {
        return Err(ApiError::ProviderUnavailable);
    }
    let response = state
        .http_client
        .post(url)
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
        .send()
        .await
        .map_err(|_| ApiError::ProviderUnavailable)?;
    if !response.status().is_success() {
        return Err(ApiError::ProviderUnavailable);
    }
    let response = response
        .json::<RpcResponse<T>>()
        .await
        .map_err(|_| ApiError::ProviderUnavailable)?;
    if response.error.is_some() {
        return Err(ApiError::ProviderUnavailable);
    }
    response.result.ok_or(ApiError::ProviderUnavailable)
}

fn valid_rpc_url(url: &reqwest::Url) -> bool {
    let host = url.host_str().unwrap_or_default();
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    url.username().is_empty()
        && url.password().is_none()
        && (url.scheme() == "https" || (url.scheme() == "http" && loopback))
        && url.host_str().is_some()
}

fn provider_name(base_url: &str) -> String {
    format!("Solana RPC ({})", provider_host(base_url))
}

fn provider_host(base_url: &str) -> String {
    reqwest::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "endpoint".into())
}

fn parse_supported_transfers(
    transaction: &SolanaTransaction,
    signature: &str,
    tracked_address: &str,
) -> Vec<ObservedNativeTransfer> {
    let Some(meta) = transaction.meta.as_ref() else {
        return Vec::new();
    };
    if meta.err.is_some() {
        return Vec::new();
    }
    let token_accounts = token_account_metadata(transaction, meta);
    let mut instructions = transaction
        .transaction
        .message
        .instructions
        .iter()
        .collect::<Vec<_>>();
    for group in &meta.inner_instructions {
        instructions.extend(group.instructions.iter());
    }

    instructions
        .into_iter()
        .filter_map(|instruction| {
            parse_system_transfer(instruction, tracked_address)
                .or_else(|| parse_spl_transfer(instruction, tracked_address, &token_accounts))
        })
        .enumerate()
        .map(|(index, mut transfer)| {
            transfer.event_index = i64::try_from(index).unwrap_or(i64::MAX);
            let block_time = transaction
                .block_time
                .and_then(|seconds| DateTime::from_timestamp(seconds, 0));
            ObservedNativeTransfer {
                signature: signature.to_owned(),
                block_height: i64::try_from(transaction.slot).ok(),
                block_time,
                transfer,
            }
        })
        .collect()
}

fn parse_system_transfer(
    instruction: &ParsedInstruction,
    tracked_address: &str,
) -> Option<NativeTransfer> {
    if instruction.program.as_deref() != Some("system") {
        return None;
    }
    let parsed = instruction.parsed.as_ref()?;
    let instruction_type = parsed.get("type")?.as_str()?;
    if !matches!(instruction_type, "transfer" | "transferWithSeed") {
        return None;
    }
    let info = parsed.get("info")?;
    let source = info.get("source")?.as_str()?;
    let destination = info.get("destination")?.as_str()?;
    let lamports = info.get("lamports")?.as_u64()?;
    if lamports == 0 {
        return None;
    }

    let direction = if source == tracked_address {
        "outbound"
    } else if destination == tracked_address {
        "inbound"
    } else {
        return None;
    };
    Some(NativeTransfer {
        event_index: -1,
        direction,
        received_amount: if direction == "inbound" { lamports } else { 0 },
        spent_amount: if direction == "outbound" { lamports } else { 0 },
        transfer_kind: "solana_native",
        asset: "SOL".into(),
        token_contract_address: None,
        token_decimals: Some(9),
        from_address: source.to_owned(),
        to_address: destination.to_owned(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TokenAccountMetadata {
    owner: String,
    mint: String,
    decimals: i16,
}

fn token_account_metadata(
    transaction: &SolanaTransaction,
    meta: &TransactionMeta,
) -> HashMap<String, TokenAccountMetadata> {
    meta.pre_token_balances
        .iter()
        .chain(&meta.post_token_balances)
        .filter_map(|balance| {
            let key = transaction
                .transaction
                .message
                .account_keys
                .get(balance.account_index)
                .and_then(account_key)
                .filter(|key| valid_public_key(key))?;
            let owner = balance.owner.as_ref()?.clone();
            if !valid_public_key(&owner) || !valid_public_key(&balance.mint) {
                return None;
            }
            Some((
                key,
                TokenAccountMetadata {
                    owner,
                    mint: balance.mint.clone(),
                    decimals: i16::from(balance.ui_token_amount.decimals),
                },
            ))
        })
        .collect()
}

fn account_key(value: &Value) -> Option<String> {
    value
        .as_str()
        .or_else(|| value.get("pubkey")?.as_str())
        .map(str::to_owned)
}

fn valid_public_key(value: &str) -> bool {
    bs58::decode(value)
        .into_vec()
        .is_ok_and(|bytes| bytes.len() == 32)
}

fn parse_spl_transfer(
    instruction: &ParsedInstruction,
    tracked_address: &str,
    token_accounts: &HashMap<String, TokenAccountMetadata>,
) -> Option<NativeTransfer> {
    if !matches!(
        instruction.program.as_deref(),
        Some("spl-token" | "spl-token-2022")
    ) {
        return None;
    }
    let parsed = instruction.parsed.as_ref()?;
    let instruction_type = parsed.get("type")?.as_str()?;
    if !matches!(instruction_type, "transfer" | "transferChecked") {
        return None;
    }
    let info = parsed.get("info")?;
    let source = info.get("source")?.as_str()?;
    let destination = info.get("destination")?.as_str()?;
    let source_metadata = token_accounts.get(source)?;
    let destination_metadata = token_accounts.get(destination)?;
    if source_metadata.mint != destination_metadata.mint {
        return None;
    }
    let mint = info
        .get("mint")
        .and_then(Value::as_str)
        .unwrap_or(&source_metadata.mint);
    if mint != source_metadata.mint || !valid_public_key(mint) {
        return None;
    }
    let token_amount = info.get("tokenAmount");
    let raw_amount = token_amount
        .and_then(|value| value.get("amount"))
        .and_then(Value::as_str)
        .or_else(|| info.get("amount").and_then(Value::as_str))?
        .parse::<u64>()
        .ok()?;
    if raw_amount == 0 {
        return None;
    }
    let decimals = token_amount
        .and_then(|value| value.get("decimals"))
        .and_then(Value::as_i64)
        .and_then(|value| i16::try_from(value).ok())
        .or(Some(source_metadata.decimals));
    let source_owner = &source_metadata.owner;
    let destination_owner = &destination_metadata.owner;
    let direction = if source_owner == tracked_address && destination_owner == tracked_address {
        "both"
    } else if source_owner == tracked_address {
        "outbound"
    } else if destination_owner == tracked_address {
        "inbound"
    } else {
        return None;
    };

    Some(NativeTransfer {
        event_index: -1,
        direction,
        received_amount: if matches!(direction, "inbound" | "both") {
            raw_amount
        } else {
            0
        },
        spent_amount: if matches!(direction, "outbound" | "both") {
            raw_amount
        } else {
            0
        },
        transfer_kind: "solana_spl",
        asset: mint.to_owned(),
        token_contract_address: Some(mint.to_owned()),
        token_decimals: decimals,
        from_address: source_owner.to_owned(),
        to_address: destination_owner.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ParsedInstruction, SolanaTransaction, parse_supported_transfers, parse_system_transfer,
        provider_name, rpc_is_configured, valid_rpc_url,
    };

    const TRACKED: &str = "So11111111111111111111111111111111111111112";

    #[test]
    fn extracts_only_native_system_transfers_for_the_tracked_key() {
        let transaction: SolanaTransaction = serde_json::from_value(json!({
            "slot": 123,
            "blockTime": 1700000000,
            "transaction": { "message": { "instructions": [
                { "program": "system", "parsed": { "type": "transfer", "info": {
                    "source": TRACKED, "destination": "11111111111111111111111111111111", "lamports": 2500000000_u64
                } } },
                { "program": "spl-token", "parsed": { "type": "transfer", "info": {
                    "source": TRACKED, "destination": "TokenAccount1111111111111111111111111111111", "amount": "5"
                } } }
            ] } },
            "meta": { "err": null, "innerInstructions": [] }
        }))
        .unwrap();

        let transfers = parse_supported_transfers(&transaction, "signature", TRACKED);
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].signature, "signature");
        assert_eq!(transfers[0].transfer.direction, "outbound");
        assert_eq!(transfers[0].transfer.spent_amount, 2_500_000_000);
        assert_eq!(transfers[0].transfer.token_decimals, Some(9));
        assert_eq!(transfers[0].transfer.from_address, TRACKED);
    }

    #[test]
    fn includes_inner_system_transfers_and_deduplicates_by_event_position() {
        let transaction: SolanaTransaction = serde_json::from_value(json!({
            "slot": 124,
            "blockTime": null,
            "transaction": { "message": { "instructions": [] } },
            "meta": { "err": null, "innerInstructions": [{ "index": 0, "instructions": [
                { "program": "system", "parsed": { "type": "transfer", "info": {
                    "source": "11111111111111111111111111111111", "destination": TRACKED, "lamports": 500000000
                } } }
            ] }] }
        }))
        .unwrap();

        let transfers = parse_supported_transfers(&transaction, "signature", TRACKED);
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].transfer.direction, "inbound");
        assert_eq!(transfers[0].transfer.received_amount, 500_000_000);
        assert_eq!(transfers[0].transfer.event_index, 0);
    }

    #[test]
    fn extracts_parsed_spl_transfer_and_transfer_checked_with_account_owners() {
        let transaction: SolanaTransaction = serde_json::from_value(json!({
            "slot": 125,
            "blockTime": 1700000000,
            "transaction": { "message": {
                "accountKeys": [
                    { "pubkey": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA" },
                    { "pubkey": "11111111111111111111111111111111" }
                ],
                "instructions": [{ "program": "spl-token", "parsed": {
                    "type": "transferChecked",
                    "info": {
                        "source": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                        "destination": "11111111111111111111111111111111",
                        "mint": "So11111111111111111111111111111111111111112",
                        "tokenAmount": { "amount": "12500000", "decimals": 9 }
                    }
                } }]
            } },
            "meta": {
                "err": null,
                "innerInstructions": [{ "index": 0, "instructions": [
                    { "program": "spl-token", "parsed": {
                        "type": "transfer",
                        "info": {
                            "source": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                            "destination": "11111111111111111111111111111111",
                            "amount": "2500000"
                        }
                    } }
                ] }],
                "preTokenBalances": [
                    { "accountIndex": 0, "mint": "So11111111111111111111111111111111111111112", "owner": TRACKED, "uiTokenAmount": { "amount": "15000000", "decimals": 9 } },
                    { "accountIndex": 1, "mint": "So11111111111111111111111111111111111111112", "owner": "11111111111111111111111111111111", "uiTokenAmount": { "amount": "0", "decimals": 9 } }
                ],
                "postTokenBalances": []
            }
        }))
        .unwrap();

        let transfers = parse_supported_transfers(&transaction, "signature", TRACKED);
        assert_eq!(transfers.len(), 2);
        assert!(
            transfers
                .iter()
                .all(|item| item.transfer.transfer_kind == "solana_spl")
        );
        assert!(
            transfers
                .iter()
                .all(|item| item.transfer.direction == "outbound")
        );
        assert_eq!(transfers[0].transfer.token_decimals, Some(9));
        assert_eq!(
            transfers[0].transfer.token_contract_address.as_deref(),
            Some("So11111111111111111111111111111111111111112")
        );
        assert_eq!(transfers[0].transfer.spent_amount, 12_500_000);
        assert_eq!(transfers[1].transfer.spent_amount, 2_500_000);
        assert_eq!(transfers[0].transfer.event_index, 0);
        assert_eq!(transfers[1].transfer.event_index, 1);
    }

    #[test]
    fn ignores_failed_transactions_and_zero_lamport_transfers() {
        let failed: SolanaTransaction = serde_json::from_value(json!({
            "slot": 1,
            "transaction": { "message": { "instructions": [] } },
            "meta": { "err": { "InstructionError": [0, "Custom"] } }
        }))
        .unwrap();
        assert!(parse_supported_transfers(&failed, "signature", TRACKED).is_empty());

        let zero = serde_json::from_value::<ParsedInstruction>(json!({
            "program": "system",
            "parsed": { "type": "transfer", "info": {
                "source": TRACKED, "destination": "11111111111111111111111111111111", "lamports": 0
            } }
        }))
        .unwrap();
        assert!(parse_system_transfer(&zero, TRACKED).is_none());
    }

    #[test]
    fn rpc_endpoint_requires_https_except_for_loopback_development() {
        for value in [
            "https://api.mainnet.solana.com",
            "http://127.0.0.1:8899",
            "http://localhost:8899",
        ] {
            assert!(valid_rpc_url(&reqwest::Url::parse(value).unwrap()));
        }
        for value in [
            "http://api.mainnet.solana.com",
            "http://192.0.2.1:8899",
            "ftp://api.mainnet.solana.com",
        ] {
            assert!(!valid_rpc_url(&reqwest::Url::parse(value).unwrap()));
        }
        assert_eq!(
            provider_name("https://rpc.example.org/secret-query?api-key=hidden"),
            "Solana RPC (rpc.example.org)"
        );
        assert!(!rpc_is_configured("not a URL"));
    }
}
