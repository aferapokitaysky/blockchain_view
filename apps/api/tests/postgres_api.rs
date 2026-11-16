use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
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
async fn network_constraint_accepts_legacy_and_valid_eip155_ids(pool: PgPool) {
    let investigation_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO investigations (name) VALUES ('Network constraint') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("investigation should be inserted");

    for network in ["ethereum", "bitcoin", "base", "arbitrum", "eip155:10"] {
        sqlx::query("INSERT INTO tracked_addresses (investigation_id, network, address) VALUES ($1, $2, 'address')")
            .bind(investigation_id)
            .bind(network)
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("network {network} should be accepted: {error}"));
    }
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

fn test_state(database: PgPool) -> ApiState {
    ApiState {
        database,
        redis: RedisClient::open("redis://127.0.0.1:1/")
            .expect("тестовый Redis URL должен разбираться"),
        http_client: reqwest::Client::new(),
        bitcoin_esplora_url: "https://blockstream.info/api".into(),
        blockscout: tracefold_api::evm_sync::BlockscoutClient::new(
            "http://127.0.0.1:1",
            None,
            reqwest::Client::new(),
        )
        .expect("тестовый Blockscout URL должен разбираться"),
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
