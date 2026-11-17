use axum::{
    Json, Router,
    body::Body,
    http::{Method, Request, StatusCode},
    routing::post,
};
use redis::Client as RedisClient;
use serde::Deserialize;
use sqlx::PgPool;
use tower::ServiceExt;
use tracefold_api::{ApiState, app};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct Investigation {
    id: Uuid,
    name: String,
}

#[derive(Debug, Deserialize)]
struct TrackedAddress {
    id: Uuid,
    network: String,
    address: String,
    label: Option<String>,
}

#[sqlx::test(migrations = "./migrations")]
async fn investigation_and_address_routes_round_trip_through_postgres(pool: PgPool) {
    let state = test_state(pool);
    let response = send(
        state.clone(),
        Method::POST,
        "/api/v1/investigations",
        Some(r#"{"name":"Case 17"}"#),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let investigation: Investigation = response_json(response).await;
    assert_eq!(investigation.name, "Case 17");

    let response = send(
        state.clone(),
        Method::POST,
        &format!("/api/v1/investigations/{}/addresses", investigation.id),
        Some(r#"{"network":"base","address":"0x0123456789ABCDEF0123456789ABCDEF01234567","label":"  Treasury  "}"#),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let tracked: TrackedAddress = response_json(response).await;
    assert_eq!(tracked.network, "base");
    assert_eq!(
        tracked.address,
        "0x0123456789abcdef0123456789abcdef01234567"
    );
    assert_eq!(tracked.label.as_deref(), Some("Treasury"));

    let response = send(
        state.clone(),
        Method::GET,
        &format!("/api/v1/investigations/{}/addresses", investigation.id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let addresses: Vec<TrackedAddress> = response_json(response).await;
    assert_eq!(addresses.len(), 1);
    assert_eq!(addresses[0].id, tracked.id);

    let response = send(
        state.clone(),
        Method::POST,
        &format!("/api/v1/investigations/{}/addresses", investigation.id),
        Some(r#"{"network":"base","address":"0x0123456789abcdef0123456789abcdef01234567"}"#),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let response = send(
        state,
        Method::GET,
        &format!("/api/v1/investigations/{}/addresses", Uuid::new_v4()),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn network_constraint_accepts_legacy_eip155_solana_tron_and_xrpl_ids(pool: PgPool) {
    let investigation_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO investigations (name) VALUES ('Network constraint') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("investigation should be inserted");

    for network in [
        "ethereum",
        "bitcoin",
        "base",
        "arbitrum",
        "eip155:10",
        "solana",
        "tron",
        "xrpl",
    ] {
        sqlx::query("INSERT INTO tracked_addresses (investigation_id, network, address) VALUES ($1, $2, 'address')")
            .bind(investigation_id)
            .bind(network)
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("network {network} should be accepted: {error}"));
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn xrpl_sync_persists_rpc_amount_strings_as_numeric(pool: PgPool) {
    let investigation_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO investigations (name) VALUES ('XRPL numeric persistence') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("исследование должно создаться");
    let address = "rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh";
    let address_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO tracked_addresses (investigation_id, network, address) VALUES ($1, 'xrpl', $2) RETURNING id",
    )
    .bind(investigation_id)
    .bind(address)
    .fetch_one(&pool)
    .await
    .expect("XRPL адрес должен сохраниться");

    let row = serde_json::json!({
        "validated": true,
        "ledger_index": 100,
        "close_time": 800000000,
        "tx_json": {
            "hash": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "TransactionType": "Payment",
            "Account": "rSender",
            "Destination": address,
            "Fee": "12",
            "Amount": "9999999"
        },
        "meta": {
            "TransactionResult": "tesSUCCESS",
            "delivered_amount": "2500000",
            "AffectedNodes": [
                {"ModifiedNode":{"LedgerEntryType":"AccountRoot","FinalFields":{"Account":address,"Balance":"32500000"},"PreviousFields":{"Balance":"30000000"}}},
                {"ModifiedNode":{"LedgerEntryType":"AccountRoot","FinalFields":{"Account":"rSender","Balance":"47499988"},"PreviousFields":{"Balance":"50000000"}}}
            ]
        }
    });
    let provider = Router::new().route(
        "/",
        post(move || {
            let row = row.clone();
            async move { Json(serde_json::json!({"result":{"transactions":[row]}})) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("локальный RPC должен запуститься");
    let provider_url = format!("http://{}/", listener.local_addr().unwrap());
    let provider_task = tokio::spawn(async move {
        axum::serve(listener, provider)
            .await
            .expect("локальный RPC должен обслуживать запросы");
    });

    let mut state = test_state(pool.clone());
    state.xrpl_rpc_url = provider_url;
    let response = send(
        state,
        Method::POST,
        &format!("/api/v1/investigations/{investigation_id}/addresses/{address_id}/sync"),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let sync_result: serde_json::Value = response_json(response).await;
    assert_eq!(sync_result["saved"], 1);

    let amounts = sqlx::query_as::<_, (String, String, String)>(
        "SELECT received_amount::text, spent_amount::text, transaction_hash FROM observed_transfers WHERE tracked_address_id = $1",
    )
    .bind(address_id)
    .fetch_one(&pool)
    .await
    .expect("сохранённый перевод должен читаться");
    assert_eq!(amounts.0, "2500000");
    assert_eq!(amounts.1, "0");
    assert_eq!(amounts.2, "a".repeat(64));
    provider_task.abort();
}

#[sqlx::test(migrations = "./migrations")]
async fn network_constraint_rejects_malformed_or_unsupported_ids(pool: PgPool) {
    let investigation_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO investigations (name) VALUES ('Invalid network') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("investigation should be inserted");

    for network in [
        "eip155:0",
        "eip155:01",
        "eip155:18446744073709551616",
        "eip155:-1",
        "solana-mainnet",
        "tron-mainnet",
        "polygon",
    ] {
        let result = sqlx::query("INSERT INTO tracked_addresses (investigation_id, network, address) VALUES ($1, $2, 'address')")
            .bind(investigation_id)
            .bind(network)
            .execute(&pool)
            .await;
        assert!(result.is_err(), "network {network} should be rejected");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn solana_spl_mint_constraint_accepts_base58_mints_and_rejects_invalid_values(pool: PgPool) {
    let investigation_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO investigations (name) VALUES ('Solana mint constraint') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("investigation should be inserted");
    let address_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO tracked_addresses (investigation_id, network, address) \
         VALUES ($1, 'solana', 'So11111111111111111111111111111111111111112') RETURNING id",
    )
    .bind(investigation_id)
    .fetch_one(&pool)
    .await
    .expect("Solana address should be inserted");

    let mint = "So11111111111111111111111111111111111111112";
    sqlx::query(
        "INSERT INTO observed_transfers \
         (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, \
          asset, token_contract_address, token_decimals, confirmed, provider, source_host) \
         VALUES ($1, 'solana', 'signature-valid', 0, 'solana_spl', 'inbound', $2, $2, 9, TRUE, 'Solana RPC', 'rpc.example.org')",
    )
    .bind(address_id)
    .bind(mint)
    .execute(&pool)
    .await
    .expect("canonical Solana mint should be accepted");

    let invalid = sqlx::query(
        "INSERT INTO observed_transfers \
         (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, \
          asset, token_contract_address, token_decimals, confirmed, provider, source_host) \
         VALUES ($1, 'solana', 'signature-invalid', 0, 'solana_spl', 'inbound', 'invalid', '0OIl', 9, TRUE, 'Solana RPC', 'rpc.example.org')",
    )
    .bind(address_id)
    .execute(&pool)
    .await;
    assert!(invalid.is_err(), "invalid Solana mint must be rejected");
}

#[sqlx::test(migrations = "./migrations")]
async fn tron_trc20_events_in_one_transaction_remain_distinct(pool: PgPool) {
    let investigation_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO investigations (name) VALUES ('TRON event identities') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("investigation should be inserted");
    let address_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO tracked_addresses (investigation_id, network, address) \
         VALUES ($1, 'tron', 'TUoHaVjx7n5xz8LwPRDckgFrDWhMhuSuJM') RETURNING id",
    )
    .bind(investigation_id)
    .fetch_one(&pool)
    .await
    .expect("TRON address should be inserted");
    let mint = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t";
    for (event_index, amount) in [(101, "1000000"), (202, "2000000")] {
        sqlx::query(
            "INSERT INTO observed_transfers \
             (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, \
              received_amount, asset, token_contract_address, confirmed, provider, source_host) \
             VALUES ($1, 'tron', 'aabbcc', $2, 'trc20', 'inbound', $3::numeric, 'USDT', $4, TRUE, 'TronGrid', 'api.trongrid.io')",
        )
        .bind(address_id)
        .bind(event_index)
        .bind(amount)
        .bind(mint)
        .execute(&pool)
        .await
        .expect("different TRC-20 events from one transaction should coexist");
    }
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM observed_transfers WHERE tracked_address_id = $1 AND transaction_hash = 'aabbcc'",
    )
    .bind(address_id)
    .fetch_one(&pool)
    .await
    .expect("TRC-20 event count should be queryable");
    assert_eq!(count, 2);
}

fn test_state(database: PgPool) -> ApiState {
    ApiState {
        database,
        redis: RedisClient::open("redis://127.0.0.1:1/")
            .expect("тестовый Redis URL должен разбираться"),
        http_client: reqwest::Client::new(),
        bitcoin_esplora_url: "https://blockstream.info/api".into(),
        solana_rpc_url: "http://127.0.0.1:1".into(),
        xrpl_rpc_url: "http://127.0.0.1:1".into(),
        blockscout: tracefold_api::evm_sync::BlockscoutClient::new(
            "http://127.0.0.1:1",
            None,
            reqwest::Client::new(),
        )
        .expect("тестовый Blockscout URL должен разбираться"),
        tron_grid: tracefold_api::tron_sync::TronGridClient::new(
            "http://127.0.0.1:1",
            None,
            reqwest::Client::new(),
        )
        .expect("тестовый TronGrid URL должен разбираться"),
    }
}

async fn send(
    state: ApiState,
    method: Method,
    uri: &str,
    body: Option<&str>,
) -> axum::response::Response {
    app(state)
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(body.map_or_else(Body::empty, |json| Body::from(json.to_owned())))
                .expect("тестовый HTTP-запрос должен собираться"),
        )
        .await
        .expect("маршрут должен вернуть HTTP-ответ")
}

async fn response_json<T: for<'de> Deserialize<'de>>(response: axum::response::Response) -> T {
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("ответ должен читаться");
    serde_json::from_slice(&body).expect("ответ должен содержать ожидаемый JSON")
}
