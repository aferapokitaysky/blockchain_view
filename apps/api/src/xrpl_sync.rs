use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{ApiState, error::ApiError, transfers::SyncResult};

const PAGE_LIMIT: usize = 50;
const MAX_HEAD_PAGES_PER_SYNC: usize = 2;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const RIPPLE_EPOCH_SECONDS: i64 = 946_684_800;

#[derive(Deserialize)]
struct RpcEnvelope {
    result: Option<Value>,
    error: Option<Value>,
}

#[derive(Debug)]
struct Transfer {
    hash: String,
    event_index: i64,
    kind: &'static str,
    direction: &'static str,
    received: String,
    spent: String,
    asset: String,
    decimals: i16,
    from: String,
    to: String,
    ledger: i64,
    time: Option<DateTime<Utc>>,
}

pub(crate) async fn sync_tracked_address(
    state: &ApiState,
    address_id: Uuid,
    address: &str,
) -> Result<SyncResult, ApiError> {
    let cursor = sqlx::query_as::<_, (Option<String>, Option<Value>, Option<Value>, bool)>(
        "SELECT head_transaction_hash, head_pending_marker, backfill_marker, backfill_exhausted FROM xrpl_sync_cursors WHERE tracked_address_id = $1",
    ).bind(address_id).fetch_optional(&state.database).await?;
    let head_hash = cursor.as_ref().and_then(|cursor| cursor.0.clone());
    let mut head_pending_marker = cursor.as_ref().and_then(|cursor| cursor.1.clone());
    let mut backfill_marker = cursor.as_ref().and_then(|cursor| cursor.2.clone());
    let mut backfill_exhausted = cursor.as_ref().is_some_and(|cursor| cursor.3);

    let mut transfers = Vec::new();
    let mut initial_page_marker: Option<Value> = None;
    let mut pages = 0;
    let mut fetched = 0;
    let mut reached_head = head_hash.is_none();
    let mut observed_head = None;
    let mut marker = None;
    for page_index in 0..MAX_HEAD_PAGES_PER_SYNC {
        let (rows, response_marker) = fetch_page(state, address, marker.as_ref()).await?;
        if page_index == 0 {
            initial_page_marker = response_marker.clone();
        }
        fetched += rows.len();
        pages += 1;
        for row in rows {
            let row_hash = transaction_hash(&row);
            if observed_head.is_none() {
                observed_head = row_hash.clone();
            }
            if is_head_checkpoint(&row, head_hash.as_deref()) {
                reached_head = true;
                break;
            }
            transfers.extend(parse_transaction(&row, address));
        }
        if reached_head {
            break;
        }
        marker = response_marker;
        if marker.is_none() {
            break;
        }
    }

    if head_hash.is_some() && !head_boundary_reached(reached_head, marker.is_some()) {
        // История провайдера закончилась до старого checkpoint: фиксируем доступную границу.
        reached_head = true;
    }

    if head_hash.is_some() && !reached_head {
        if let Some(pending) = head_pending_marker.take() {
            let (rows, next) = fetch_page(state, address, Some(&pending)).await?;
            fetched += rows.len();
            pages += 1;
            for row in rows {
                if is_head_checkpoint(&row, head_hash.as_deref()) {
                    reached_head = true;
                    break;
                }
                transfers.extend(parse_transaction(&row, address));
            }
            if !head_boundary_reached(reached_head, next.is_some()) {
                reached_head = true;
            }
            head_pending_marker = if reached_head { None } else { next };
        } else {
            head_pending_marker = marker.clone();
            if !head_boundary_reached(reached_head, head_pending_marker.is_some()) {
                reached_head = true;
            }
        }
    }

    // Head scan always starts from the newest ledger; backfill advances independently.
    if head_hash.is_none() {
        backfill_marker = initial_page_marker;
        backfill_exhausted = backfill_marker.is_none();
    } else if !backfill_exhausted {
        if let Some(marker) = backfill_marker.take() {
            let (rows, next) = fetch_page(state, address, Some(&marker)).await?;
            fetched += rows.len();
            pages += 1;
            for row in rows {
                transfers.extend(parse_transaction(&row, address));
            }
            backfill_marker = next;
            backfill_exhausted = backfill_marker.is_none();
        } else {
            backfill_exhausted = true;
        }
    }

    let provider = format!("XRPL JSON-RPC ({})", provider_host(&state.xrpl_rpc_url));
    let host = provider_host(&state.xrpl_rpc_url);
    let observed_at = Utc::now();
    let mut transaction = state.database.begin().await?;
    for transfer in &transfers {
        sqlx::query(
            "INSERT INTO observed_transfers \
             (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, \
              received_amount, spent_amount, asset, token_contract_address, token_decimals, \
              from_address, to_address, block_height, block_time, confirmed, provider, source_host, observed_at) \
             VALUES ($1, 'xrpl', $2, $3, $4, $5, $6::NUMERIC, $7::NUMERIC, $8, $9, $10, $11, $12, $13, $14, TRUE, $15, $16, $17) \
             ON CONFLICT (tracked_address_id, transaction_hash, transfer_kind, event_index) DO UPDATE SET \
              direction = EXCLUDED.direction, received_amount = EXCLUDED.received_amount, spent_amount = EXCLUDED.spent_amount, \
              asset = EXCLUDED.asset, token_contract_address = EXCLUDED.token_contract_address, \
              token_decimals = EXCLUDED.token_decimals, from_address = EXCLUDED.from_address, to_address = EXCLUDED.to_address, \
              block_height = EXCLUDED.block_height, block_time = EXCLUDED.block_time, confirmed = TRUE, \
              provider = EXCLUDED.provider, source_host = EXCLUDED.source_host, observed_at = EXCLUDED.observed_at",
        )
        .bind(address_id).bind(&transfer.hash).bind(transfer.event_index).bind(transfer.kind)
        .bind(transfer.direction).bind(&transfer.received).bind(&transfer.spent).bind(&transfer.asset)
        .bind(Option::<String>::None).bind(transfer.decimals).bind(&transfer.from).bind(&transfer.to)
        .bind(transfer.ledger).bind(transfer.time).bind(&provider).bind(&host).bind(observed_at)
        .execute(&mut *transaction).await?;
    }
    if reached_head {
        head_pending_marker = None;
    }
    let new_head = next_head_hash(head_hash, observed_head, reached_head);
    sqlx::query(
        "INSERT INTO xrpl_sync_cursors (tracked_address_id, head_transaction_hash, head_pending_marker, backfill_marker, backfill_exhausted, updated_at) VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (tracked_address_id) DO UPDATE SET head_transaction_hash = EXCLUDED.head_transaction_hash, head_pending_marker = EXCLUDED.head_pending_marker, backfill_marker = EXCLUDED.backfill_marker, backfill_exhausted = EXCLUDED.backfill_exhausted, updated_at = EXCLUDED.updated_at",
    ).bind(address_id).bind(new_head).bind(head_pending_marker).bind(backfill_marker).bind(backfill_exhausted).bind(observed_at).execute(&mut *transaction).await?;
    transaction.commit().await?;

    Ok(SyncResult {
        provider,
        fetched,
        saved: transfers.len(),
        pages,
        may_have_more_history: !reached_head || !backfill_exhausted,
        observed_at,
    })
}

fn transaction_hash(row: &Value) -> Option<String> {
    let hash = row.get("tx_json").or_else(|| row.get("tx"))?["hash"]
        .as_str()
        .or(row["hash"].as_str())?;
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(hash.to_ascii_lowercase())
}

fn is_head_checkpoint(row: &Value, checkpoint: Option<&str>) -> bool {
    let Some(checkpoint) = checkpoint else {
        return false;
    };
    let checkpoint = checkpoint.to_ascii_lowercase();
    transaction_hash(row).as_deref() == Some(checkpoint.as_str())
}

fn next_head_hash(
    previous: Option<String>,
    observed: Option<String>,
    reached_checkpoint: bool,
) -> Option<String> {
    if reached_checkpoint {
        observed.or(previous)
    } else {
        previous
    }
}

fn head_boundary_reached(checkpoint_found: bool, has_more_pages: bool) -> bool {
    checkpoint_found || !has_more_pages
}

async fn fetch_page(
    state: &ApiState,
    address: &str,
    marker: Option<&Value>,
) -> Result<(Vec<Value>, Option<Value>), ApiError> {
    let mut params = json!({
        "account": address,
        "ledger_index_min": -1,
        "ledger_index_max": -1,
        "binary": false,
        "forward": false,
        "limit": PAGE_LIMIT,
    });
    if let Some(marker) = marker {
        params["marker"] = marker.clone();
    }
    let result = rpc_call(state, params).await?;
    let rows = result
        .get("transactions")
        .and_then(Value::as_array)
        .cloned()
        .ok_or(ApiError::ProviderUnavailable)?;
    let marker = result
        .get("marker")
        .filter(|marker| !marker.is_null())
        .cloned();
    Ok((rows, marker))
}

async fn rpc_call(state: &ApiState, params: Value) -> Result<Value, ApiError> {
    let url =
        reqwest::Url::parse(&state.xrpl_rpc_url).map_err(|_| ApiError::ProviderUnavailable)?;
    let mut response = state
        .http_client
        .post(url)
        .json(&json!({"method":"account_tx", "params":[params]}))
        .timeout(std::time::Duration::from_secs(12))
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
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ApiError::ProviderUnavailable)?
    {
        if !append_response_chunk(&mut body, &chunk) {
            return Err(ApiError::ProviderUnavailable);
        }
    }
    let envelope =
        serde_json::from_slice::<RpcEnvelope>(&body).map_err(|_| ApiError::ProviderUnavailable)?;
    if envelope.error.is_some() {
        return Err(ApiError::ProviderUnavailable);
    }
    let result = envelope.result.ok_or(ApiError::ProviderUnavailable)?;
    if result
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status != "success")
    {
        return Err(ApiError::ProviderUnavailable);
    }
    Ok(result)
}

fn append_response_chunk(body: &mut Vec<u8>, chunk: &[u8]) -> bool {
    if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
        return false;
    }
    body.extend_from_slice(chunk);
    true
}

fn parse_transaction(row: &Value, tracked: &str) -> Vec<Transfer> {
    if row.get("validated").and_then(Value::as_bool) != Some(true) {
        return vec![];
    }
    let meta = &row["meta"];
    if meta["TransactionResult"].as_str() != Some("tesSUCCESS") {
        return vec![];
    }
    let tx = row
        .get("tx_json")
        .or_else(|| row.get("tx"))
        .unwrap_or(&Value::Null);
    let Some(hash) = transaction_hash(row) else {
        return vec![];
    };
    let ledger = row["ledger_index"].as_i64().unwrap_or_default();
    if ledger <= 0 {
        return vec![];
    }
    let time = row["close_time_iso"]
        .as_str()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
        .or_else(|| {
            row["close_time"]
                .as_i64()
                .and_then(|seconds| seconds.checked_add(RIPPLE_EPOCH_SECONDS))
                .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
        });
    let tx_account = tx["Account"].as_str().unwrap_or_default();
    let tx_destination = tx["Destination"].as_str().unwrap_or_default();
    if tx["TransactionType"] != "Payment"
        || tx.get("Paths").is_some()
        || tx_account.is_empty()
        || tx_destination.is_empty()
        || tx_account == tx_destination
        || (tracked != tx_account && tracked != tx_destination)
    {
        return vec![];
    }

    // delivered_amount исключает частичный Payment, где заявленная сумма отличается от фактически полученной.
    let delivered = &meta["delivered_amount"];
    let Some(drops) = delivered.as_str() else {
        return vec![];
    };
    let Some(amount) = drops.parse::<u128>().ok().filter(|amount| *amount > 0) else {
        return vec![];
    };
    let Some(amount_for_check) = i128::try_from(amount).ok() else {
        return vec![];
    };
    let Some((balance_delta, account_created)) = account_root_delta(meta, tracked) else {
        return vec![];
    };
    if tracked == tx_account {
        let Some(fee) = tx["Fee"].as_str().and_then(|fee| fee.parse::<i128>().ok()) else {
            return vec![];
        };
        let Some(spent) = balance_delta
            .checked_neg()
            .and_then(|spent| spent.checked_sub(fee))
        else {
            return vec![];
        };
        if spent != amount_for_check {
            return vec![];
        }
    } else if !account_created && balance_delta != amount_for_check {
        return vec![];
    }

    let (direction, from, to) = if tracked == tx_destination {
        ("inbound", tx_account, tx_destination)
    } else {
        ("outbound", tx_account, tx_destination)
    };
    vec![Transfer {
        hash,
        event_index: 0,
        kind: "xrpl_native",
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
        asset: "XRP".to_owned(),
        decimals: 6,
        from: from.to_owned(),
        to: to.to_owned(),
        ledger,
        time,
    }]
}

fn account_root_delta(meta: &Value, tracked: &str) -> Option<(i128, bool)> {
    for affected in meta["AffectedNodes"].as_array()? {
        if let Some(node) = affected.get("ModifiedNode") {
            if node["LedgerEntryType"] == "AccountRoot"
                && node["FinalFields"]["Account"].as_str() == Some(tracked)
            {
                let before = node["PreviousFields"]["Balance"]
                    .as_str()?
                    .parse::<i128>()
                    .ok()?;
                let after = node["FinalFields"]["Balance"]
                    .as_str()?
                    .parse::<i128>()
                    .ok()?;
                return Some((after.checked_sub(before)?, false));
            }
        }
        if let Some(node) = affected.get("CreatedNode") {
            if node["LedgerEntryType"] == "AccountRoot"
                && node["NewFields"]["Account"].as_str() == Some(tracked)
            {
                let balance = node["NewFields"]["Balance"]
                    .as_str()?
                    .parse::<i128>()
                    .ok()?;
                return Some((balance, true));
            }
        }
    }
    None
}

fn provider_host(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "endpoint".into())
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_RESPONSE_BYTES, append_response_chunk, head_boundary_reached, is_head_checkpoint,
        next_head_hash, parse_transaction, transaction_hash,
    };
    use serde_json::json;

    #[test]
    fn parses_successful_payment_without_counting_the_fee_as_xrp_transfer() {
        let row = json!({"validated":true,"ledger_index":100,"close_time":800000000,"tx_json":{"hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","TransactionType":"Payment","Account":"rSender","Destination":"rTracked","Fee":"12","Amount":"9999999"},"meta":{"TransactionResult":"tesSUCCESS","delivered_amount":"2500000","AffectedNodes":[{"ModifiedNode":{"LedgerEntryType":"AccountRoot","FinalFields":{"Account":"rTracked","Balance":"32500000"},"PreviousFields":{"Balance":"30000000"}}},{"ModifiedNode":{"LedgerEntryType":"AccountRoot","FinalFields":{"Account":"rSender","Balance":"47499988"},"PreviousFields":{"Balance":"50000000"}}}]}});
        let transfers = parse_transaction(&row, "rTracked");
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].received, "2500000");
        assert_eq!(transfers[0].spent, "0");
        assert_eq!(transfers[0].kind, "xrpl_native");
        let sent = parse_transaction(&row, "rSender");
        assert_eq!(sent[0].spent, "2500000");
        assert_eq!(sent[0].received, "0");
    }

    #[test]
    fn ignores_failed_and_unvalidated_transactions() {
        let mut row = json!({"validated":false,"ledger_index":100,"tx_json":{"hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"},"meta":{"TransactionResult":"tesSUCCESS"}});
        assert!(parse_transaction(&row, "rTracked").is_empty());
        row["validated"] = json!(true);
        assert!(parse_transaction(&row, "rTracked").is_empty());
    }

    #[test]
    fn skips_issued_currency_payments_until_the_asset_model_is_explicit() {
        let row = json!({"validated":true,"ledger_index":100,"tx_json":{"hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","TransactionType":"Payment","Account":"rSender","Destination":"rTracked"},"meta":{"TransactionResult":"tesSUCCESS","delivered_amount":{"currency":"USD","issuer":"rIssuer","value":"1.25"}}});
        assert!(parse_transaction(&row, "rTracked").is_empty());
    }

    #[test]
    fn skips_path_payments_and_non_payment_balance_changes() {
        let mut row = json!({"validated":true,"ledger_index":100,"tx_json":{"hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","TransactionType":"Payment","Account":"rSender","Destination":"rTracked","Paths":[[]]},"meta":{"TransactionResult":"tesSUCCESS","delivered_amount":"100"}});
        assert!(parse_transaction(&row, "rTracked").is_empty());
        row["tx_json"]["TransactionType"] = json!("OfferCreate");
        row["tx_json"].as_object_mut().unwrap().remove("Paths");
        assert!(parse_transaction(&row, "rTracked").is_empty());
    }

    #[test]
    fn uses_delivered_amount_for_new_account_without_counting_its_reserve() {
        let row = json!({"validated":true,"ledger_index":100,"tx_json":{"hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","TransactionType":"Payment","Account":"rSender","Destination":"rTracked","Fee":"12"},"meta":{"TransactionResult":"tesSUCCESS","delivered_amount":"2500000","AffectedNodes":[{"CreatedNode":{"LedgerEntryType":"AccountRoot","NewFields":{"Account":"rTracked","Balance":"2501000"}}},{"ModifiedNode":{"LedgerEntryType":"AccountRoot","FinalFields":{"Account":"rSender","Balance":"47499988"},"PreviousFields":{"Balance":"50000000"}}}]}});
        let transfers = parse_transaction(&row, "rTracked");
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].received, "2500000");
    }

    #[test]
    fn rejects_payment_when_account_root_delta_disagrees_with_delivery() {
        let row = json!({"validated":true,"ledger_index":100,"tx_json":{"hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","TransactionType":"Payment","Account":"rSender","Destination":"rTracked","Fee":"12"},"meta":{"TransactionResult":"tesSUCCESS","delivered_amount":"2500000","AffectedNodes":[{"ModifiedNode":{"LedgerEntryType":"AccountRoot","FinalFields":{"Account":"rTracked","Balance":"32499999"},"PreviousFields":{"Balance":"30000000"}}}]}});
        assert!(parse_transaction(&row, "rTracked").is_empty());
    }

    #[test]
    fn new_head_transactions_are_processed_until_the_previous_checkpoint() {
        let new = json!({"tx_json":{"hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}});
        let checkpoint = json!({"tx_json":{"hash":"BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"}});
        assert!(!is_head_checkpoint(
            &new,
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        ));
        assert!(is_head_checkpoint(
            &checkpoint,
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        ));
        assert!(transaction_hash(&json!({"hash":"not-a-hash"})).is_none());
        assert_eq!(
            transaction_hash(&checkpoint),
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into())
        );
    }

    #[test]
    fn head_checkpoint_advances_only_after_the_previous_head_is_reached() {
        assert_eq!(
            next_head_hash(Some("OLD".into()), Some("NEW".into()), false),
            Some("OLD".into())
        );
        assert_eq!(
            next_head_hash(Some("OLD".into()), Some("NEW".into()), true),
            Some("NEW".into())
        );
    }

    #[test]
    fn stale_head_checkpoint_stops_when_provider_history_is_exhausted() {
        let available_head = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let next_marker: Option<serde_json::Value> = None;
        let reached_head = head_boundary_reached(false, next_marker.is_some());
        assert!(reached_head);
        assert_eq!(
            next_head_hash(
                Some("pruned-checkpoint".into()),
                Some(available_head.into()),
                reached_head
            ),
            Some(available_head.into())
        );
    }

    #[test]
    fn response_body_limit_is_enforced_before_appending_the_oversized_chunk() {
        let mut body = vec![0; MAX_RESPONSE_BYTES];
        assert!(!append_response_chunk(&mut body, &[1]));
        assert_eq!(body.len(), MAX_RESPONSE_BYTES);
    }
}
