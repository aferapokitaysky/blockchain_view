use axum::{
    Router,
    http::{Method, header},
    routing::get,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

pub mod addresses;
pub mod config;
pub mod error;
pub mod health;
pub mod investigations;
pub mod transfers;

pub type ApiState = health::HealthState;

pub fn app(state: ApiState) -> Router {
    Router::new()
        .route("/health/live", get(health::live))
        .route("/health/ready", get(health::ready))
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
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin([
                    axum::http::HeaderValue::from_static("http://localhost:5173"),
                    axum::http::HeaderValue::from_static("http://127.0.0.1:5173"),
                ])
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers([header::CONTENT_TYPE]),
        )
        .layer(TraceLayer::new_for_http())
}
