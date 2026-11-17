use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{Method, header},
    routing::get,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

pub mod addresses;
pub mod config;
pub mod error;
pub mod evm_sync;
pub mod graph_snapshots;
pub mod health;
pub mod investigations;
pub mod networks;
pub mod risk_signals;
pub mod sanctions;
pub mod solana_sync;
pub mod sync_worker;
pub mod transfers;
pub mod tron_sync;
pub mod xrpl_sync;

pub type ApiState = health::HealthState;

pub fn app(state: ApiState) -> Router {
    let risk_signal_routes = Router::new()
        .route(
            "/api/v1/investigations/{investigation_id}/addresses/{address_id}/risk-signals",
            get(risk_signals::list).post(risk_signals::create),
        )
        .route(
            "/api/v1/investigations/{investigation_id}/addresses/{address_id}/risk-signals/{signal_id}",
            get(risk_signals::get_one)
                .put(risk_signals::update)
                .delete(risk_signals::delete),
        )
        .layer(DefaultBodyLimit::max(risk_signals::MAX_REQUEST_BYTES));
    let graph_snapshot_routes = Router::new()
        .route(
            "/api/v1/investigations/{investigation_id}/graph-snapshots",
            get(graph_snapshots::list).post(graph_snapshots::create),
        )
        .layer(DefaultBodyLimit::max(graph_snapshots::MAX_REQUEST_BYTES));

    Router::new()
        .route("/health/live", get(health::live))
        .route("/health/ready", get(health::ready))
        .route("/api/v1/networks", get(networks::list))
        .route(
            "/api/v1/investigations",
            get(investigations::list).post(investigations::create),
        )
        .route(
            "/api/v1/investigations/{investigation_id}/addresses",
            get(addresses::list).post(addresses::create),
        )
        .route(
            "/api/v1/investigations/{investigation_id}/addresses/{address_id}/sync",
            axum::routing::post(transfers::sync_address),
        )
        .route(
            "/api/v1/investigations/{investigation_id}/addresses/{address_id}/transfers",
            get(transfers::list_address_transfers),
        )
        .route("/api/v1/screening/address", get(sanctions::screen_address))
        .merge(risk_signal_routes)
        .merge(graph_snapshot_routes)
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin([
                    axum::http::HeaderValue::from_static("http://localhost:5173"),
                    axum::http::HeaderValue::from_static("http://127.0.0.1:5173"),
                ])
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers([header::CONTENT_TYPE]),
        )
        .layer(TraceLayer::new_for_http())
}
