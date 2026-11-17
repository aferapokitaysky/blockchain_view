use axum::{body::Body, http::Request};
use redis::Client as RedisClient;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
use tracefold_api::{
    ApiState, app,
    sanctions::{import_ofac_snapshot, parse_ofac_sdn_xml},
};

const SDN_FIXTURE: &str = include_str!("fixtures/ofac-sdn.xml");

#[sqlx::test(migrations = "./migrations")]
async fn exact_address_screening_uses_the_imported_snapshot_and_reports_network_coverage(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = parse_ofac_sdn_xml(SDN_FIXTURE)?;
    assert_eq!(import_ofac_snapshot(&pool, snapshot).await?, 2);
    let app = app(test_state(pool));

    let exact_match = request(
        app.clone(),
        "/api/v1/screening/address?network=ethereum&address=0xAbCd000000000000000000000000000000000001",
    )
    .await;
    assert_eq!(exact_match.status(), axum::http::StatusCode::OK);
    let match_body = response_json(exact_match).await;
    assert_eq!(match_body["status"], "exact_match");
    assert_eq!(match_body["match_type"], "exact_address");
    assert_eq!(
        match_body["source"]["source_name"],
        "OFAC Specially Designated Nationals List"
    );
    assert_eq!(
        match_body["matches"][0]["address"],
        "0xAbCd000000000000000000000000000000000001"
    );
    assert_eq!(match_body["matches"][0]["entity_uid"], "12345");
    assert_eq!(match_body["matches"][0]["programs"][0], "TEST-PROGRAM");

    let no_match = request(
        app.clone(),
        "/api/v1/screening/address?network=ethereum&address=0x9999999999999999999999999999999999999999",
    )
    .await;
    let no_match_body = response_json(no_match).await;
    assert_eq!(no_match_body["status"], "no_exact_match");
    assert_eq!(no_match_body["matches"].as_array().map(Vec::len), Some(0));
    assert_eq!(no_match_body["source"]["address_count"], 2);

    let unsupported = request(
        app,
        "/api/v1/screening/address?network=base&address=0x9999999999999999999999999999999999999999",
    )
    .await;
    let unsupported_body = response_json(unsupported).await;
    assert_eq!(unsupported_body["status"], "network_not_covered");
    assert_eq!(unsupported_body["source"], Value::Null);
    Ok(())
}

fn test_state(database: PgPool) -> ApiState {
    ApiState {
        database,
        redis: RedisClient::open("redis://127.0.0.1:1/")
            .expect("тестовый Redis URL должен разбираться"),
        http_client: reqwest::Client::new(),
        bitcoin_esplora_url: "http://127.0.0.1:1/api".into(),
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

async fn request(app: axum::Router, uri: &str) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("тестовый запрос должен собираться"),
    )
    .await
    .expect("маршрут должен вернуть HTTP-ответ")
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("тестовый ответ должен читаться");
    serde_json::from_slice(&bytes).expect("ответ должен содержать JSON")
}
