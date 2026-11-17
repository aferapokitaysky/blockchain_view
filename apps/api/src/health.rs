use std::time::Duration;

use axum::{Json, extract::State, http::StatusCode};
use redis::Client as RedisClient;
use serde::Serialize;
use sqlx::PgPool;
use tokio::time::timeout;
use tracing::warn;

use crate::aptos_sync::AptosClient;
use crate::cardano_sync::CardanoClient;
use crate::etherscan_sync::EtherscanClient;
use crate::evm_sync::BlockscoutClient;
use crate::sui_sync::SuiClient;
use crate::ton_sync::TonCenterClient;
use crate::tron_sync::TronGridClient;

#[derive(Clone)]
pub struct HealthState {
    pub database: PgPool,
    pub redis: RedisClient,
    pub http_client: reqwest::Client,
    pub bitcoin_esplora_url: String,
    pub solana_rpc_url: String,
    pub solana_helius_api_key: Option<String>,
    pub xrpl_rpc_url: String,
    pub blockscout: BlockscoutClient,
    pub etherscan: EtherscanClient,
    pub tron_grid: TronGridClient,
    pub ton_center: TonCenterClient,
    pub cardano: CardanoClient,
    pub aptos: AptosClient,
    pub sui: SuiClient,
}

#[cfg(test)]
pub(crate) fn unavailable_state() -> HealthState {
    HealthState {
        database: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://tracefold:tracefold@127.0.0.1:1/tracefold")
            .expect("тестовый адрес Postgres должен разбираться"),
        redis: RedisClient::open("redis://127.0.0.1:1/")
            .expect("тестовый адрес Redis должен разбираться"),
        http_client: reqwest::Client::new(),
        bitcoin_esplora_url: "http://127.0.0.1:1/api".into(),
        solana_rpc_url: "http://127.0.0.1:1".into(),
        solana_helius_api_key: None,
        xrpl_rpc_url: "http://127.0.0.1:1".into(),
        blockscout: BlockscoutClient::new("http://127.0.0.1:1", None, reqwest::Client::new())
            .expect("тестовая конфигурация Blockscout должна разбираться"),
        etherscan: EtherscanClient::new(None, false, reqwest::Client::new())
            .expect("тестовая конфигурация Etherscan должна разбираться"),
        tron_grid: TronGridClient::new("http://127.0.0.1:1", None, reqwest::Client::new())
            .expect("тестовая конфигурация TronGrid должна разбираться"),
        ton_center: TonCenterClient::new("http://127.0.0.1:1", None, reqwest::Client::new())
            .expect("тестовая конфигурация TON Center должна разбираться"),
        cardano: CardanoClient::new(None, reqwest::Client::new(), None)
            .expect("тестовая конфигурация Cardano должна разбираться"),
        aptos: AptosClient::new("https://127.0.0.1/graphql", None, reqwest::Client::new())
            .expect("тестовая конфигурация Aptos должна разбираться"),
        sui: SuiClient::new(
            "https://127.0.0.1/graphql",
            None,
            reqwest::Client::new(),
            None,
        )
        .expect("тестовая конфигурация Sui должна разбираться"),
    }
}

#[derive(Serialize)]
pub(crate) struct LiveResponse {
    status: &'static str,
}

#[derive(Serialize)]
pub(crate) struct ReadyResponse {
    status: &'static str,
    dependencies: DependencyStatus,
}

#[derive(Serialize)]
pub(crate) struct DependencyStatus {
    postgres: &'static str,
    redis: &'static str,
}

pub(crate) async fn live() -> Json<LiveResponse> {
    Json(LiveResponse { status: "live" })
}

pub(crate) async fn ready(State(state): State<HealthState>) -> (StatusCode, Json<ReadyResponse>) {
    let (postgres_ok, redis_ok) =
        tokio::join!(check_postgres(&state.database), check_redis(&state.redis));
    let response = ReadyResponse {
        status: if postgres_ok && redis_ok {
            "ready"
        } else {
            "not_ready"
        },
        dependencies: DependencyStatus {
            postgres: if postgres_ok { "ready" } else { "unavailable" },
            redis: if redis_ok { "ready" } else { "unavailable" },
        },
    };

    if !postgres_ok {
        warn!(dependency = "postgres", "проверка готовности не пройдена");
    }
    if !redis_ok {
        warn!(dependency = "redis", "проверка готовности не пройдена");
    }

    let status = if postgres_ok && redis_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(response))
}

async fn check_postgres(pool: &PgPool) -> bool {
    timeout(
        Duration::from_secs(3),
        sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(pool),
    )
    .await
    .is_ok_and(|result| result.is_ok())
}

async fn check_redis(client: &RedisClient) -> bool {
    let check = async {
        let mut connection = client.get_multiplexed_async_connection().await?;
        let reply: String = redis::cmd("PING").query_async(&mut connection).await?;
        Ok::<bool, redis::RedisError>(reply == "PONG")
    };
    timeout(Duration::from_secs(3), check)
        .await
        .is_ok_and(|result| result.is_ok_and(|reply| reply))
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use super::unavailable_state;
    use crate::app;

    #[tokio::test]
    async fn live_does_not_depend_on_storage_availability() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/health/live")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn ready_fails_when_dependencies_are_unavailable() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/health/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
