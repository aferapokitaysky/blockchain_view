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
async fn graph_snapshot_is_saved_and_rejects_observations_from_another_case(pool: PgPool) {
    let state = test_state(pool);
    let first = create_investigation(&state, "Graph case").await;
    let second = create_investigation(&state, "Other case").await;
    let first_address = create_address(&state, first.id).await;
    let second_address = create_address(&state, second.id).await;
    let first_transfer = Uuid::new_v4();
    let second_transfer = Uuid::new_v4();
    for (id, address_id) in [
        (first_transfer, first_address.id),
        (second_transfer, second_address.id),
    ] {
        sqlx::query(
            "INSERT INTO observed_transfers \
             (id, tracked_address_id, network, transaction_hash, direction, asset, confirmed, provider, \
              event_index, transfer_kind, from_address, to_address) \
             VALUES ($1, $2, 'tron', $3, 'outbound', 'TRX', TRUE, 'TronGrid', 0, 'tron_native', $4, $5)",
        )
        .bind(id)
        .bind(address_id)
        .bind(format!("tx-{id}"))
        .bind("TUoHaVjx7n5xz8LwPRDckgFrDWhMhuSuJM")
        .bind("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t")
        .execute(&state.database)
        .await
        .unwrap();
    }

    let body = serde_json::json!({
        "title": "TRON flow",
        "focus_network": "tron",
        "focus_address": "TUoHaVjx7n5xz8LwPRDckgFrDWhMhuSuJM",
        "max_depth": 2,
        "source_transfer_ids": [first_transfer],
        "graph": {"nodes": [], "edges": [], "truncated": false, "sourceTransferIds": [first_transfer]},
    });
    let body = body.to_string();
    let response = send(
        state.clone(),
        Method::POST,
        &format!("/api/v1/investigations/{}/graph-snapshots", first.id),
        Some(&body),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let snapshot: serde_json::Value = response_json(response).await;
    assert_eq!(snapshot["title"], "TRON flow");
    assert_eq!(
        snapshot["source_transfer_ids"][0],
        first_transfer.to_string()
    );

    let response = send(
        state.clone(),
        Method::GET,
        &format!("/api/v1/investigations/{}/graph-snapshots", first.id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let snapshots: Vec<serde_json::Value> = response_json(response).await;
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0]["graph"]["nodes"], serde_json::json!([]));

    let invalid_body = serde_json::json!({
        "title": "Cross-case source",
        "focus_network": null,
        "focus_address": null,
        "max_depth": 1,
        "source_transfer_ids": [second_transfer],
        "graph": {"nodes": [], "edges": []},
    });
    let invalid_body = invalid_body.to_string();
    let response = send(
        state,
        Method::POST,
        &format!("/api/v1/investigations/{}/graph-snapshots", first.id),
        Some(&invalid_body),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrations = "./migrations")]
async fn cardano_history_returns_observed_transfers_and_participant_relations(pool: PgPool) {
    let state = test_state(pool.clone());
    let investigation = create_investigation(&state, "Cardano history response").await;
    let tracked_address = "addr1qx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3n0d3vllmyqwsx5wktcd8cc3sq835lu7drv2xwl2wywfgse35a3x";
    let counterparty = "Ae2tdPwUPEZLs4HtbuNey7tK4hTKrwNwYtGqp7bDfCy2WdR3P6735W5Yfpe";
    let address_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO tracked_addresses (investigation_id, network, address) \
         VALUES ($1, 'cardano', $2) RETURNING id",
    )
    .bind(investigation.id)
    .bind(tracked_address)
    .fetch_one(&pool)
    .await
    .unwrap();
    let transaction_hash = "a".repeat(64);
    sqlx::query(
        "INSERT INTO observed_transfers \
         (tracked_address_id, network, transaction_hash, event_index, transfer_kind, direction, \
          received_amount, spent_amount, asset, token_decimals, from_address, to_address, confirmed, provider, source_host) \
         VALUES ($1, 'cardano', $2, 0, 'cardano_native', 'both', 250000, 1000000, 'ADA', 6, NULL, NULL, TRUE, 'Blockfrost', 'cardano-mainnet.blockfrost.io')",
    )
    .bind(address_id)
    .bind(&transaction_hash)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO observed_relationships \
         (tracked_address_id, network, transaction_hash, counterparty_address, relationship_type, \
          participant_role, block_height, block_time, confirmed, provider, source_host) \
         VALUES ($1, 'cardano', $2, $3, 'same_transaction', 'input', 42, now(), TRUE, 'Blockfrost', 'cardano-mainnet.blockfrost.io')",
    )
    .bind(address_id)
    .bind(&transaction_hash)
    .bind(counterparty)
    .execute(&pool)
    .await
    .unwrap();

    let response = send(
        state,
        Method::GET,
        &format!(
            "/api/v1/investigations/{}/addresses/{address_id}/history",
            investigation.id
        ),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let history: serde_json::Value = response_json(response).await;
    assert_eq!(history["transfers"].as_array().unwrap().len(), 1);
    assert_eq!(
        history["transfers"][0]["from_address"],
        serde_json::Value::Null
    );
    assert_eq!(history["transfers"][0]["received_amount"], "250000");
    assert_eq!(history["relationships"].as_array().unwrap().len(), 1);
    assert_eq!(
        history["relationships"][0]["counterparty_address"],
        counterparty
    );
    assert_eq!(
        history["relationships"][0]["relationship_type"],
        "same_transaction"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn graph_snapshot_persists_only_cardano_relationships_from_its_investigation(pool: PgPool) {
    let state = test_state(pool.clone());
    let first = create_investigation(&state, "Cardano graph case").await;
    let second = create_investigation(&state, "Other Cardano case").await;
    let first_address = create_cardano_address(&pool, first.id).await;
    let second_address = create_cardano_address(&pool, second.id).await;
    let first_relationship = insert_cardano_relationship(&pool, first_address.id).await;
    let second_relationship = insert_cardano_relationship(&pool, second_address.id).await;
    let body = serde_json::json!({
        "title": "Cardano participants",
        "focus_network": "cardano",
        "focus_address": first_address.address,
        "max_depth": 2,
        "source_transfer_ids": [],
        "source_relationship_ids": [first_relationship],
        "graph": {"nodes": [], "edges": [], "truncated": false, "sourceRelationshipIds": [first_relationship]},
    });
    let body = body.to_string();
    let response = send(
        state.clone(),
        Method::POST,
        &format!("/api/v1/investigations/{}/graph-snapshots", first.id),
        Some(&body),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let snapshot: serde_json::Value = response_json(response).await;
    assert_eq!(
        snapshot["source_relationship_ids"][0],
        first_relationship.to_string()
    );

    let response = send(
        state.clone(),
        Method::GET,
        &format!("/api/v1/investigations/{}/graph-snapshots", first.id),
        None,
    )
    .await;
    let snapshots: Vec<serde_json::Value> = response_json(response).await;
    assert_eq!(
        snapshots[0]["source_relationship_ids"][0],
        first_relationship.to_string()
    );

    let invalid = serde_json::json!({
        "title": "Cross-case relationship",
        "focus_network": null,
        "focus_address": null,
        "max_depth": 1,
        "source_transfer_ids": [],
        "source_relationship_ids": [second_relationship],
        "graph": {"nodes": [], "edges": []},
    });
    let invalid = invalid.to_string();
    let response = send(
        state,
        Method::POST,
        &format!("/api/v1/investigations/{}/graph-snapshots", first.id),
        Some(&invalid),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

async fn create_cardano_address(pool: &PgPool, investigation_id: Uuid) -> TrackedAddress {
    let address = "addr1qx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3n0d3vllmyqwsx5wktcd8cc3sq835lu7drv2xwl2wywfgse35a3x";
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO tracked_addresses (investigation_id, network, address) \
         VALUES ($1, 'cardano', $2) RETURNING id",
    )
    .bind(investigation_id)
    .bind(address)
    .fetch_one(pool)
    .await
    .unwrap();
    TrackedAddress {
        id,
        network: "cardano".into(),
        address: address.into(),
        label: None,
    }
}

async fn insert_cardano_relationship(pool: &PgPool, tracked_address_id: Uuid) -> Uuid {
    sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO observed_relationships \
         (tracked_address_id, network, transaction_hash, counterparty_address, relationship_type, \
          participant_role, confirmed, provider, source_host) \
         VALUES ($1, 'cardano', $2, $3, 'same_transaction', 'output', TRUE, 'Blockfrost', 'cardano-mainnet.blockfrost.io') \
         RETURNING id",
    )
    .bind(tracked_address_id)
    .bind(Uuid::new_v4().simple().to_string().repeat(2))
    .bind("Ae2tdPwUPEZLs4HtbuNey7tK4hTKrwNwYtGqp7bDfCy2WdR3P6735W5Yfpe")
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn create_investigation(state: &ApiState, name: &str) -> Investigation {
    let body = serde_json::json!({"name": name});
    let body = body.to_string();
    let response = send(
        state.clone(),
        Method::POST,
        "/api/v1/investigations",
        Some(&body),
    )
    .await;
    response_json(response).await
}

async fn create_address(state: &ApiState, investigation_id: Uuid) -> TrackedAddress {
    let body = serde_json::json!({
        "network": "tron",
        "address": "TUoHaVjx7n5xz8LwPRDckgFrDWhMhuSuJM"
    });
    let body = body.to_string();
    let response = send(
        state.clone(),
        Method::POST,
        &format!("/api/v1/investigations/{investigation_id}/addresses"),
        Some(&body),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    response_json(response).await
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
        etherscan: tracefold_api::etherscan_sync::EtherscanClient::new(
            None,
            false,
            reqwest::Client::new(),
        )
        .expect("тестовая конфигурация Etherscan должна разбираться"),
        tron_grid: tracefold_api::tron_sync::TronGridClient::new(
            "http://127.0.0.1:1",
            None,
            reqwest::Client::new(),
        )
        .expect("тестовый TronGrid URL должен разбираться"),
        ton_center: tracefold_api::ton_sync::TonCenterClient::new(
            "http://127.0.0.1:1",
            None,
            reqwest::Client::new(),
        )
        .expect("тестовый TON Center URL должен разбираться"),
        cardano: tracefold_api::cardano_sync::CardanoClient::from_env(
            reqwest::Client::new(),
            RedisClient::open("redis://127.0.0.1:1/").unwrap(),
        )
        .expect("тестовая конфигурация Cardano должна разбираться"),
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
