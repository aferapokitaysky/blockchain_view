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

fn test_state(database: PgPool) -> ApiState {
    ApiState {
        database,
        redis: RedisClient::open("redis://127.0.0.1:1/")
            .expect("тестовый Redis URL должен разбираться"),
        http_client: reqwest::Client::new(),
        bitcoin_esplora_url: "https://blockstream.info/api".into(),
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
