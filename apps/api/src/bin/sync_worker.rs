use std::{error::Error, time::Duration};

use sqlx::postgres::PgPoolOptions;
use tokio::sync::watch;
use tracefold_api::{evm_sync::BlockscoutClient, health::HealthState, sync_worker};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")?;
    let redis_url = std::env::var("REDIS_URL")?;
    let database = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&database).await?;
    let redis = redis::Client::open(redis_url)?;
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("Tracefold/0.1 (+https://github.com/aferapokitaysky/blockchain_view)")
        .build()?;
    let bitcoin_esplora_url = std::env::var("BITCOIN_ESPLORA_URL")
        .unwrap_or_else(|_| "https://blockstream.info/api".into());
    let solana_rpc_url =
        std::env::var("SOLANA_RPC_URL").unwrap_or_else(|_| "https://api.mainnet.solana.com".into());
    let blockscout = BlockscoutClient::from_env(http_client.clone())?;
    let state = HealthState {
        database,
        redis,
        http_client,
        bitcoin_esplora_url,
        solana_rpc_url,
        blockscout,
    };

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let signal_tx = shutdown_tx.clone();
    let signal_task = tokio::spawn(async move {
        shutdown_signal().await;
        let _ = signal_tx.send(true);
    });
    info!("воркер фоновой синхронизации запущен");
    sync_worker::run(state, shutdown_rx).await?;
    let _ = shutdown_tx.send(true);
    signal_task.abort();
    info!("воркер фоновой синхронизации остановлен");
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
        _ = ctrl_c => warn!("получен сигнал Ctrl+C"),
        _ = terminate => warn!("получен SIGTERM"),
    }
}
