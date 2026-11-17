use std::{error::Error, time::Duration};

use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use tracefold_api::{
    app, config::Config, etherscan_sync::EtherscanClient, evm_sync::BlockscoutClient,
    health::HealthState, ton_sync::TonCenterClient, tron_sync::TronGridClient,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env()?;
    let database = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(Duration::from_secs(3))
        .connect_lazy(&config.database_url)?;
    sqlx::migrate!("./migrations").run(&database).await?;
    let redis = redis::Client::open(config.redis_url)?;
    let listener = TcpListener::bind(config.bind_address).await?;
    info!(address = %config.bind_address, "API запущен");
    warn!("API пока не защищён аутентификацией и предназначен только для локальной разработки");

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Tracefold/0.1 (+https://github.com/aferapokitaysky/blockchain_view)")
        .build()?;
    let bitcoin_esplora_url = std::env::var("BITCOIN_ESPLORA_URL")
        .unwrap_or_else(|_| "https://blockstream.info/api".into());
    let solana_rpc_url =
        std::env::var("SOLANA_RPC_URL").unwrap_or_else(|_| "https://api.mainnet.solana.com".into());
    let xrpl_rpc_url =
        std::env::var("XRPL_RPC_URL").unwrap_or_else(|_| "https://s2.ripple.com:51234/".into());
    let blockscout = BlockscoutClient::from_env(http_client.clone())?;
    let etherscan = EtherscanClient::from_env(http_client.clone(), redis.clone())?;
    let tron_grid = TronGridClient::from_env(http_client.clone())?;
    let ton_center = TonCenterClient::from_env(http_client.clone())?;

    axum::serve(
        listener,
        app(HealthState {
            database,
            redis,
            http_client,
            bitcoin_esplora_url,
            solana_rpc_url,
            xrpl_rpc_url,
            blockscout,
            etherscan,
            tron_grid,
            ton_center,
        }),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("не удалось обработать Ctrl+C");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("не удалось зарегистрировать SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("получен сигнал Ctrl+C, завершаем работу"),
        _ = terminate => info!("получен SIGTERM, завершаем работу"),
    }
}
