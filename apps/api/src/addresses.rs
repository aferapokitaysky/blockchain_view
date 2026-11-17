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
    Tron,
}

impl Network {
    pub(crate) fn normalize(self, value: &str) -> Result<String, ApiError> {
        let value = value.trim();
        match self {
            Self::Ethereum | Self::Base | Self::Arbitrum => normalize_evm_address(value),
            Self::Tron => normalize_tron_address(value),
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
    let network = crate::networks::provider_catalog(&state)
        .await
        .into_iter()
        .find(|network| network.id == input.network)
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
        crate::networks::NetworkFamily::Utxo if network.id == "cardano" => {
            normalize_cardano_address(value)
        }
        crate::networks::NetworkFamily::Utxo => Err(ApiError::UnsupportedNetwork),
        crate::networks::NetworkFamily::Solana if network.id == "solana" => {
            normalize_solana_address(value)
        }
        crate::networks::NetworkFamily::Solana => Err(ApiError::UnsupportedNetwork),
        crate::networks::NetworkFamily::Tron if network.id == "tron" => {
            normalize_tron_address(value)
        }
        crate::networks::NetworkFamily::Tron => Err(ApiError::UnsupportedNetwork),
        crate::networks::NetworkFamily::Xrpl if network.id == "xrpl" => {
            normalize_xrpl_address(value)
        }
        crate::networks::NetworkFamily::Xrpl => Err(ApiError::UnsupportedNetwork),
        crate::networks::NetworkFamily::Ton if network.id == "ton" => normalize_ton_address(value),
        crate::networks::NetworkFamily::Ton => Err(ApiError::UnsupportedNetwork),
        crate::networks::NetworkFamily::Aptos if network.id == "aptos" => {
            normalize_aptos_address(value)
        }
        crate::networks::NetworkFamily::Aptos => Err(ApiError::UnsupportedNetwork),
        crate::networks::NetworkFamily::Sui if network.id == "sui" => {
            crate::sui_sync::normalize_sui_address(value)
                .ok_or(ApiError::Validation("Неверный формат адреса Sui"))
        }
        crate::networks::NetworkFamily::Sui => Err(ApiError::UnsupportedNetwork),
    }
}

pub(crate) fn normalize_aptos_address(value: &str) -> Result<String, ApiError> {
    let address = value
        .trim()
        .strip_prefix("0x")
        .or_else(|| value.trim().strip_prefix("0X"))
        .unwrap_or(value.trim());
    if address.is_empty()
        || address.len() > 64
        || !address.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ApiError::Validation("Неверный формат адреса Aptos"));
    }
    Ok(format!("0x{:0>64}", address.to_ascii_lowercase()))
}

pub(crate) fn normalize_ton_address(value: &str) -> Result<String, ApiError> {
    use base64::{
        Engine,
        engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
    };
    let address = value.trim();
    if let Some((workchain, account)) = address.split_once(':') {
        if matches!(workchain, "0" | "-1")
            && account.len() == 64
            && account.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Ok(format!("{workchain}:{}", account.to_ascii_lowercase()));
        }
        return Err(ApiError::Validation("Неверный raw-адрес TON"));
    }
    let decoded = [URL_SAFE, URL_SAFE_NO_PAD, STANDARD, STANDARD_NO_PAD]
        .iter()
        .find_map(|engine| engine.decode(address).ok())
        .ok_or(ApiError::Validation("Неверный friendly-адрес TON"))?;
    if decoded.len() != 36 || !matches!(decoded[0], 0x11 | 0x51) {
        return Err(ApiError::Validation(
            "Неверный friendly-адрес основной сети TON",
        ));
    }
    let checksum = crc16_xmodem(&decoded[..34]);
    if decoded[34] != (checksum >> 8) as u8 || decoded[35] != checksum as u8 {
        return Err(ApiError::Validation(
            "Неверная контрольная сумма адреса TON",
        ));
    }
    let workchain = decoded[1] as i8;
    if !matches!(workchain, 0 | -1) {
        return Err(ApiError::Validation(
            "Адрес TON использует неподдерживаемый workchain",
        ));
    }
    let account = decoded[2..34]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("{workchain}:{account}"))
}

pub(crate) fn normalize_cardano_address(value: &str) -> Result<String, ApiError> {
    use pallas_addresses::Address;

    let address = Address::from_str(value.trim())
        .map_err(|_| ApiError::Validation("Неверный формат адреса Cardano"))?;
    match address {
        Address::Shelley(address) if address.network().is_mainnet() => address
            .to_bech32()
            .map_err(|_| ApiError::Validation("Неверный Bech32-адрес Cardano")),
        Address::Byron(address) => {
            let checksum = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
            if checksum.checksum(address.payload.as_ref()) != address.crc {
                return Err(ApiError::Validation(
                    "Неверная контрольная сумма Byron-адреса Cardano",
                ));
            }
            let payload = address
                .decode()
                .map_err(|_| ApiError::Validation("Неверный Byron-адрес Cardano"))?;
            if payload.attributes.iter().any(|attribute| {
                matches!(
                    attribute,
                    pallas_addresses::byron::AddrAttrProperty::NetworkTag(_)
                )
            }) {
                return Err(ApiError::Validation(
                    "Byron-адрес Cardano не относится к основной сети",
                ));
            }
            Ok(address.to_base58())
        }
        Address::Shelley(_) | Address::Stake(_) => Err(ApiError::Validation(
            "Нужен payment-адрес основной сети Cardano",
        )),
    }
}

fn crc16_xmodem(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for byte in bytes {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod ton_address_tests {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

    use super::normalize_ton_address;

    #[test]
    fn normalizes_raw_ton_address_to_lowercase() {
        let address = format!("0:{}", "AB".repeat(32));
        assert_eq!(
            normalize_ton_address(&address).unwrap(),
            format!("0:{}", "ab".repeat(32))
        );
    }

    #[test]
    fn accepts_valid_mainnet_friendly_ton_address() {
        let mut bytes = [0u8; 36];
        bytes[0] = 0x11;
        bytes[1] = 0;
        bytes[2..34].fill(0x42);
        let crc = super::crc16_xmodem(&bytes[..34]);
        bytes[34] = (crc >> 8) as u8;
        bytes[35] = crc as u8;
        let friendly = URL_SAFE_NO_PAD.encode(bytes);
        assert_eq!(
            normalize_ton_address(&friendly).unwrap(),
            format!("0:{}", "42".repeat(32))
        );
    }

    #[test]
    fn rejects_invalid_friendly_checksum_and_testnet_flags() {
        let mut bytes = [0u8; 36];
        bytes[0] = 0x91;
        let friendly = URL_SAFE_NO_PAD.encode(bytes);
        assert!(normalize_ton_address(&friendly).is_err());
    }
}

pub(crate) fn normalize_xrpl_address(value: &str) -> Result<String, ApiError> {
    let address = value.trim();
    if address.starts_with('X') {
        return Err(ApiError::Validation(
            "X-address не поддерживается; используйте classic-адрес XRP Ledger",
        ));
    }
    let payload = bs58::decode(address)
        .with_alphabet(bs58::Alphabet::RIPPLE)
        .with_check(Some(0))
        .into_vec()
        .map_err(|_| ApiError::Validation("Неверный classic-адрес XRP Ledger"))?;
    if payload.len() != 21
        || payload[0] != 0
        || bs58::encode(&payload)
            .with_alphabet(bs58::Alphabet::RIPPLE)
            .with_check()
            .into_string()
            != address
    {
        return Err(ApiError::Validation("Неверный classic-адрес XRP Ledger"));
    }
    Ok(address.to_owned())
}

pub(crate) fn normalize_solana_address(value: &str) -> Result<String, ApiError> {
    let address = value.trim();
    let decoded = bs58::decode(address)
        .into_vec()
        .map_err(|_| ApiError::Validation("Неверный формат адреса Solana"))?;
    if decoded.len() != 32 || bs58::encode(&decoded).into_string() != address {
        return Err(ApiError::Validation("Неверный формат адреса Solana"));
    }
    Ok(address.to_owned())
}

pub(crate) fn normalize_tron_address(value: &str) -> Result<String, ApiError> {
    let address = value.trim();
    let hex = address
        .strip_prefix("0x")
        .or_else(|| address.strip_prefix("0X"))
        .unwrap_or(address);
    let address = if hex.len() == 42 && hex[..2].eq_ignore_ascii_case("41") {
        let mut payload = Vec::with_capacity(21);
        for [high, low] in hex.as_bytes().as_chunks::<2>().0 {
            let high = (*high as char).to_digit(16);
            let low = (*low as char).to_digit(16);
            let Some(byte) = high.zip(low).map(|(high, low)| high * 16 + low) else {
                return Err(ApiError::Validation("Неверный формат адреса TRON"));
            };
            payload.push(byte as u8);
        }
        bs58::encode(payload).with_check().into_string()
    } else {
        address.to_owned()
    };

    let payload = bs58::decode(&address)
        .with_check(None)
        .into_vec()
        .map_err(|_| ApiError::Validation("Неверный формат адреса TRON"))?;
    if payload.len() != 21
        || payload[0] != 0x41
        || bs58::encode(&payload).with_check().into_string() != address
    {
        return Err(ApiError::Validation("Неверный формат адреса TRON"));
    }
    Ok(address)
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
    use super::{
        Network, normalize_aptos_address, normalize_cardano_address, normalize_catalogued_address,
        normalize_solana_address, normalize_tron_address, normalize_xrpl_address,
    };
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use crate::{
        app,
        health::unavailable_state,
        networks::{NetworkFamily, NetworkInfo},
    };

    #[test]
    fn normalizes_aptos_hex_addresses_to_full_lowercase_form() {
        assert_eq!(
            normalize_aptos_address("0xAbC").unwrap(),
            format!("0x{:0>64}", "abc")
        );
        assert!(normalize_aptos_address("0x").is_err());
        assert!(normalize_aptos_address(&format!("0x{}g", "1".repeat(63))).is_err());
        assert!(normalize_aptos_address(&format!("0x{}", "1".repeat(65))).is_err());
    }

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
            requires_api_key: false,
            requires_paid_tier: false,
            history_coverage: None,
            api_base_url: Some(reqwest::Url::parse("https://explorer.optimism.io/").unwrap()),
            sync_provider: Some(crate::networks::NetworkSyncProvider::Blockscout),
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
    fn validates_solana_public_keys_as_canonical_base58_32_byte_values() {
        let address = "So11111111111111111111111111111111111111112";
        assert_eq!(normalize_solana_address(address).unwrap(), address);
        assert!(normalize_solana_address("So1111111111111111111111111111111111111111").is_err());
        assert!(normalize_solana_address("0OIl").is_err());
    }

    #[test]
    fn validates_tron_base58check_and_normalizes_hex_addresses() {
        let address = "TUoHaVjx7n5xz8LwPRDckgFrDWhMhuSuJM";
        assert_eq!(normalize_tron_address(address).unwrap(), address);
        assert_eq!(
            normalize_tron_address("41ce8a0cf0c16d48bcf22825f6053248df653c89ca").unwrap(),
            address
        );
        assert_eq!(
            normalize_tron_address("0x41ce8a0cf0c16d48bcf22825f6053248df653c89ca").unwrap(),
            address
        );
        assert!(normalize_tron_address("T1111111111111111111111111111111111").is_err());
        assert!(normalize_tron_address("00ce8a0cf0c16d48bcf22825f6053248df653c89ca").is_err());
    }

    #[test]
    fn validates_xrpl_classic_address_checksum_and_version() {
        let address = "rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh";
        assert_eq!(normalize_xrpl_address(address).unwrap(), address);
        assert!(normalize_xrpl_address("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTi").is_err());
        assert!(normalize_xrpl_address("X7EVg4kB2e7S").is_err());
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

    #[test]
    fn validates_cardano_mainnet_payment_and_byron_addresses() {
        const SHELLEY: &str = "addr1qx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3n0d3vllmyqwsx5wktcd8cc3sq835lu7drv2xwl2wywfgse35a3x";
        const BYRON: &str = "Ae2tdPwUPEZLs4HtbuNey7tK4hTKrwNwYtGqp7bDfCy2WdR3P6735W5Yfpe";

        assert_eq!(normalize_cardano_address(SHELLEY).unwrap(), SHELLEY);
        assert_eq!(normalize_cardano_address(BYRON).unwrap(), BYRON);
        assert!(
            normalize_cardano_address(
                "stake1ux3g2c9dx2nhhehyrezyxpkstartcqmu9hk63qgfkccw5rqttygt7"
            )
            .is_err()
        );
        assert!(normalize_cardano_address("not-a-cardano-address").is_err());
    }

    #[test]
    fn rejects_testnet_and_bad_byron_checksum() {
        use pallas_addresses::{Address, Network, ShelleyAddress};

        const SHELLEY: &str = "addr1qx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3n0d3vllmyqwsx5wktcd8cc3sq835lu7drv2xwl2wywfgse35a3x";
        const BYRON: &str = "Ae2tdPwUPEZLs4HtbuNey7tK4hTKrwNwYtGqp7bDfCy2WdR3P6735W5Yfpe";

        let Address::Shelley(shelley) = Address::from_bech32(SHELLEY).unwrap() else {
            panic!("fixture is a Shelley payment address");
        };
        let testnet = ShelleyAddress::new(
            Network::Testnet,
            shelley.payment().clone(),
            shelley.delegation().clone(),
        )
        .to_bech32()
        .unwrap();
        assert!(normalize_cardano_address(&testnet).is_err());

        let mut byron = pallas_addresses::ByronAddress::from_base58(BYRON).unwrap();
        byron.crc ^= 1;
        assert!(normalize_cardano_address(&byron.to_base58()).is_err());
    }

    #[test]
    fn rejects_byron_addresses_with_network_tags() {
        use pallas_addresses::byron::{
            AddrAttrProperty, AddrType, AddressPayload, ByronAddress, SpendingData,
        };

        let payload = AddressPayload::new(
            AddrType::PubKey,
            SpendingData::PubKey(vec![0; 32].into()),
            vec![AddrAttrProperty::NetworkTag(vec![1].into())].into(),
        );
        let testnet_address = ByronAddress::from_decoded(payload).to_base58();
        assert!(normalize_cardano_address(&testnet_address).is_err());
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
