use std::str::FromStr;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use bitcoin::{Address as BitcoinAddress, Network as BitcoinNetwork};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{ApiState, error::ApiError};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Ethereum,
    Bitcoin,
    Base,
    Arbitrum,
}

impl Network {
    pub(crate) fn normalize(self, value: &str) -> Result<String, ApiError> {
        let value = value.trim();
        match self {
            Self::Ethereum | Self::Base | Self::Arbitrum => normalize_evm_address(value),
            Self::Bitcoin => {
                let parsed = BitcoinAddress::from_str(value)
                    .map_err(|_| ApiError::Validation("Неверный формат адреса Bitcoin"))?;
                let checked = parsed
                    .require_network(BitcoinNetwork::Bitcoin)
                    .map_err(|_| {
                        ApiError::Validation("Адрес не относится к основной сети Bitcoin")
                    })?;
                Ok(checked.to_string())
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateAddress {
    pub network: String,
    pub address: String,
    pub label: Option<String>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct TrackedAddress {
    pub id: Uuid,
    pub investigation_id: Uuid,
    pub network: String,
    pub address: String,
    pub label: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub(crate) async fn list(
    State(state): State<ApiState>,
    Path(investigation_id): Path<Uuid>,
) -> Result<Json<Vec<TrackedAddress>>, ApiError> {
    ensure_investigation(&state, investigation_id).await?;
    let addresses = sqlx::query_as::<_, TrackedAddress>(
        "SELECT id, investigation_id, network, address, label, created_at \
         FROM tracked_addresses WHERE investigation_id = $1 ORDER BY created_at DESC",
    )
    .bind(investigation_id)
    .fetch_all(&state.database)
    .await?;
    Ok(Json(addresses))
}

pub(crate) async fn create(
    State(state): State<ApiState>,
    Path(investigation_id): Path<Uuid>,
    Json(input): Json<CreateAddress>,
) -> Result<(StatusCode, Json<TrackedAddress>), ApiError> {
    let network = state
        .blockscout
        .network_by_id(&input.network)
        .await
        .ok_or(ApiError::UnsupportedNetwork)?;
    let address = normalize_catalogued_address(&network, &input.address)?;
    let label = input
        .label
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if label
        .as_ref()
        .is_some_and(|value| value.chars().count() > 80)
    {
        return Err(ApiError::Validation(
            "Метка не должна превышать 80 символов",
        ));
    }
    ensure_investigation(&state, investigation_id).await?;

    let tracked_address = sqlx::query_as::<_, TrackedAddress>(
        "INSERT INTO tracked_addresses (investigation_id, network, address, label) \
         VALUES ($1, $2, $3, $4) RETURNING id, investigation_id, network, address, label, created_at",
    )
    .bind(investigation_id)
    .bind(network.id)
    .bind(address)
    .bind(label)
    .fetch_one(&state.database)
    .await?;

    Ok((StatusCode::CREATED, Json(tracked_address)))
}

fn normalize_catalogued_address(
    network: &crate::networks::NetworkInfo,
    value: &str,
) -> Result<String, ApiError> {
    match network.family {
        crate::networks::NetworkFamily::Evm => normalize_evm_address(value),
        crate::networks::NetworkFamily::Utxo if network.id == "bitcoin" => {
            Network::Bitcoin.normalize(value)
        }
        crate::networks::NetworkFamily::Utxo => Err(ApiError::UnsupportedNetwork),
    }
}

pub(crate) fn normalize_evm_address(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    let valid = value.len() == 42
        && value.starts_with("0x")
        && value[2..].bytes().all(|byte| byte.is_ascii_hexdigit());
    valid
        .then(|| value.to_ascii_lowercase())
        .ok_or(ApiError::Validation(
            "Для EVM-сети нужен адрес вида 0x + 40 шестнадцатеричных символов",
        ))
}

async fn ensure_investigation(state: &ApiState, id: Uuid) -> Result<(), ApiError> {
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM investigations WHERE id = $1)")
            .bind(id)
            .fetch_one(&state.database)
            .await?;
    exists.then_some(()).ok_or(ApiError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::{Network, normalize_catalogued_address};
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use crate::{
        app,
        health::unavailable_state,
        networks::{NetworkFamily, NetworkInfo},
    };

    #[test]
    fn normalizes_catalogued_evm_addresses_for_dynamic_chain_ids() {
        let network = NetworkInfo {
            id: "eip155:10".into(),
            name: "OP Mainnet".into(),
            family: NetworkFamily::Evm,
            chain_id: Some(10),
            native_asset: "ETH".into(),
            explorer_url: Some("https://explorer.optimism.io/".into()),
            history_sync_available: true,
            api_base_url: Some(reqwest::Url::parse("https://explorer.optimism.io/").unwrap()),
        };
        assert_eq!(
            normalize_catalogued_address(&network, "0x0123456789ABCDEF0123456789ABCDEF01234567")
                .unwrap(),
            "0x0123456789abcdef0123456789abcdef01234567"
        );
    }

    #[test]
    fn normalizes_evm_addresses_without_guessing_the_network() {
        let address = "0x0123456789ABCDEF0123456789ABCDEF01234567";
        assert_eq!(
            Network::Base.normalize(address).unwrap(),
            address.to_lowercase()
        );
        assert!(Network::Arbitrum.normalize("0x1234").is_err());
    }

    #[test]
    fn validates_bitcoin_mainnet_addresses() {
        assert!(
            Network::Bitcoin
                .normalize("bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh")
                .is_ok()
        );
        assert!(Network::Bitcoin.normalize("tb1qxyz").is_err());
    }

    #[tokio::test]
    async fn rejects_malformed_address_before_database_lookup() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/investigations/550e8400-e29b-41d4-a716-446655440000/addresses")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"network":"base","address":"0x1234"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
    }
}
