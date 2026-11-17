use axum::{Router, body::Body, http::Request};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use tracefold_api::{ApiState, app};

async fn request(app: Router, method: &str, uri: String, body: Value) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn state(pool: PgPool) -> ApiState {
    ApiState {
        database: pool,
        redis: redis::Client::open("redis://127.0.0.1:1/").unwrap(),
        http_client: reqwest::Client::new(),
        bitcoin_esplora_url: "http://127.0.0.1:1/api".into(),
        solana_rpc_url: "http://127.0.0.1:1".into(),
        solana_helius_api_key: None,
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
            redis::Client::open("redis://127.0.0.1:1/").unwrap(),
        )
        .expect("тестовая конфигурация Cardano должна разбираться"),
        aptos: tracefold_api::aptos_sync::AptosClient::new(
            "https://127.0.0.1/graphql",
            None,
            reqwest::Client::new(),
        )
        .expect("тестовая конфигурация Aptos должна разбираться"),
        sui: tracefold_api::sui_sync::SuiClient::new(
            "https://127.0.0.1/graphql",
            None,
            reqwest::Client::new(),
            None,
        )
        .expect("тестовая конфигурация Sui должна разбираться"),
    }
}

fn sample_signal() -> Value {
    json!({
        "signal_key": "manual_source_review",
        "category": "exposure",
        "severity": "medium",
        "source_name": "Analyst review",
        "source_reference": "case-42/evidence-2",
        "observed_at": "2026-11-15T10:00:00Z",
        "details": {"hops": 2, "note": "Manual review; no automated attribution"}
    })
}

#[sqlx::test(migrations = "./migrations")]
async fn risk_signal_crud_is_scoped_to_the_investigation_and_address(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let investigation_id = Uuid::new_v4();
    let other_investigation_id = Uuid::new_v4();
    let address_id = Uuid::new_v4();
    let other_address_id = Uuid::new_v4();

    for id in [investigation_id, other_investigation_id] {
        sqlx::query("INSERT INTO investigations (id, name) VALUES ($1, $2)")
            .bind(id)
            .bind(format!("investigation-{id}"))
            .execute(&pool)
            .await?;
    }
    sqlx::query(
        "INSERT INTO tracked_addresses (id, investigation_id, network, address) \
         VALUES ($1, $2, 'ethereum', $3), ($4, $5, 'base', $6)",
    )
    .bind(address_id)
    .bind(investigation_id)
    .bind("0x0123456789abcdef0123456789abcdef01234567")
    .bind(other_address_id)
    .bind(other_investigation_id)
    .bind("0x1123456789abcdef0123456789abcdef01234567")
    .execute(&pool)
    .await?;

    let base =
        format!("/api/v1/investigations/{investigation_id}/addresses/{address_id}/risk-signals");
    let created_response = request(
        app(state(pool.clone())),
        "POST",
        base.clone(),
        sample_signal(),
    )
    .await;
    assert_eq!(created_response.status(), axum::http::StatusCode::CREATED);
    let created = response_json(created_response).await;
    let signal_id = created["id"].as_str().unwrap();
    assert_eq!(created["source_name"], "Analyst review");
    assert_eq!(created["source_reference"], "case-42/evidence-2");
    assert_eq!(created["details"]["hops"], 2);
    assert_eq!(created["observed_at"], "2026-11-15T10:00:00Z");

    let list_response = request(app(state(pool.clone())), "GET", base.clone(), Value::Null).await;
    assert_eq!(list_response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response_json(list_response).await.as_array().unwrap().len(),
        1
    );

    let item_uri = format!("{base}/{signal_id}");
    let get_response = request(
        app(state(pool.clone())),
        "GET",
        item_uri.clone(),
        Value::Null,
    )
    .await;
    assert_eq!(get_response.status(), axum::http::StatusCode::OK);

    let mut updated_signal = sample_signal();
    updated_signal["signal_key"] = json!("revised_analyst_review");
    updated_signal["severity"] = json!("low");
    updated_signal["source_reference"] = json!("case-42/evidence-3");
    let update_response = request(
        app(state(pool.clone())),
        "PUT",
        item_uri.clone(),
        updated_signal,
    )
    .await;
    assert_eq!(update_response.status(), axum::http::StatusCode::OK);
    let updated = response_json(update_response).await;
    assert_eq!(updated["signal_key"], "revised_analyst_review");
    assert_eq!(updated["severity"], "low");

    let other_scope = format!(
        "/api/v1/investigations/{other_investigation_id}/addresses/{other_address_id}/risk-signals/{signal_id}"
    );
    for (method, uri, body) in [
        ("GET", other_scope.clone(), Value::Null),
        ("PUT", other_scope.clone(), sample_signal()),
        ("DELETE", other_scope, Value::Null),
    ] {
        let response = request(app(state(pool.clone())), method, uri, body).await;
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }

    let other_address_path = format!(
        "/api/v1/investigations/{investigation_id}/addresses/{other_address_id}/risk-signals"
    );
    let cross_address_list = request(
        app(state(pool.clone())),
        "GET",
        other_address_path.clone(),
        Value::Null,
    )
    .await;
    assert_eq!(
        cross_address_list.status(),
        axum::http::StatusCode::NOT_FOUND
    );

    let cross_address_create = request(
        app(state(pool.clone())),
        "POST",
        other_address_path,
        sample_signal(),
    )
    .await;
    assert_eq!(
        cross_address_create.status(),
        axum::http::StatusCode::NOT_FOUND
    );

    let delete_response = request(
        app(state(pool.clone())),
        "DELETE",
        item_uri.clone(),
        Value::Null,
    )
    .await;
    assert_eq!(delete_response.status(), axum::http::StatusCode::NO_CONTENT);

    let missing_response = request(app(state(pool)), "GET", item_uri, Value::Null).await;
    assert_eq!(missing_response.status(), axum::http::StatusCode::NOT_FOUND);
    Ok(())
}
