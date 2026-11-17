use std::{collections::BTreeMap, net::IpAddr, str::FromStr};

use axum::{Json, extract::State};
use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::{ApiState, error::ApiError};

pub(crate) const MAX_CATALOG_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NetworkFamily {
    Evm,
    Utxo,
    Solana,
    Tron,
    Xrpl,
    Ton,
}

#[derive(Clone, Debug, Serialize)]
pub struct NetworkInfo {
    pub id: String,
    pub name: String,
    pub family: NetworkFamily,
    pub chain_id: Option<u64>,
    pub native_asset: String,
    pub explorer_url: Option<String>,
    pub history_sync_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_coverage: Option<Vec<&'static str>>,
    #[serde(skip)]
    pub(crate) api_base_url: Option<Url>,
}

#[derive(Clone, Debug, Serialize)]
pub struct NetworkCatalogResponse {
    pub history_sync_available: bool,
    pub networks: Vec<NetworkInfo>,
}

#[derive(Debug, Deserialize)]
struct BlockscoutConfig {
    #[serde(default)]
    chains: BTreeMap<String, String>,
    #[serde(default)]
    chains_metadata: BTreeMap<String, ChainMetadata>,
}

#[derive(Debug, Deserialize)]
struct ChainMetadata {
    name: Option<String>,
    #[serde(rename = "isTestnet")]
    is_testnet: Option<bool>,
    native_currency: Option<String>,
    instance_url_from_config: Option<String>,
    explorers: Option<Vec<ExplorerMetadata>>,
}

#[derive(Debug, Deserialize)]
struct ExplorerMetadata {
    #[serde(rename = "hostedBy")]
    hosted_by: Option<String>,
    url: Option<String>,
}

pub(crate) fn parse_blockscout_config(
    body: &[u8],
    has_configured_universal_provider: bool,
) -> Result<Vec<NetworkInfo>, ()> {
    if body.len() > MAX_CATALOG_BYTES {
        return Err(());
    }
    let config: BlockscoutConfig = serde_json::from_slice(body).map_err(|_| ())?;
    let mut networks = Vec::new();
    for (chain_id, _explorer_url) in config.chains {
        let Ok(chain_id) = chain_id.parse::<u64>() else {
            continue;
        };
        if chain_id == 0 {
            continue;
        }
        let Some(metadata) = config.chains_metadata.get(&chain_id.to_string()) else {
            continue;
        };
        if metadata.is_testnet != Some(false) {
            continue;
        }
        let Some(name) = metadata
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty() && name.chars().count() <= 80)
        else {
            continue;
        };
        let native_asset = metadata
            .native_currency
            .as_deref()
            .map(str::trim)
            .filter(|asset| !asset.is_empty() && asset.chars().count() <= 16)
            .unwrap_or("Unknown");
        let api_base_url = metadata
            .instance_url_from_config
            .as_deref()
            .and_then(valid_public_https_base_url)
            .or_else(|| {
                metadata
                    .explorers
                    .as_deref()?
                    .iter()
                    .find(|explorer| explorer.hosted_by.as_deref() == Some("blockscout"))?
                    .url
                    .as_deref()
                    .and_then(valid_public_https_base_url)
            });
        let chain_explorer_url = metadata
            .explorers
            .as_deref()
            .and_then(|explorers| explorers.iter().find_map(|entry| entry.url.as_deref()))
            .and_then(valid_public_https_base_url)
            .map(|url| url.to_string());
        let id = legacy_id(chain_id)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("eip155:{chain_id}"));
        networks.push(NetworkInfo {
            id,
            name: name.to_owned(),
            family: NetworkFamily::Evm,
            chain_id: Some(chain_id),
            native_asset: native_asset.to_owned(),
            explorer_url: chain_explorer_url,
            history_sync_available: api_base_url.is_some() || has_configured_universal_provider,
            history_coverage: None,
            api_base_url,
        });
    }
    if networks.is_empty() {
        return Err(());
    }
    merge_legacy_fallbacks(&mut networks);
    networks.sort_by_key(|network| network.name.to_lowercase());
    Ok(networks)
}

pub(crate) fn fallback_catalog() -> Vec<NetworkInfo> {
    let mut networks = vec![
        static_evm(
            "ethereum",
            "Ethereum",
            1,
            "ETH",
            "https://eth.blockscout.com/",
        ),
        static_evm("base", "Base", 8453, "ETH", "https://base.blockscout.com/"),
        static_evm(
            "arbitrum",
            "Arbitrum One",
            42161,
            "ETH",
            "https://arbitrum.blockscout.com/",
        ),
        NetworkInfo {
            id: "bitcoin".into(),
            name: "Bitcoin".into(),
            family: NetworkFamily::Utxo,
            chain_id: None,
            native_asset: "BTC".into(),
            explorer_url: Some("https://blockstream.info/".into()),
            history_sync_available: true,
            history_coverage: None,
            api_base_url: None,
        },
        NetworkInfo {
            id: "solana".into(),
            name: "Solana".into(),
            family: NetworkFamily::Solana,
            chain_id: None,
            native_asset: "SOL".into(),
            explorer_url: Some("https://explorer.solana.com/".into()),
            history_sync_available: true,
            history_coverage: None,
            api_base_url: None,
        },
        NetworkInfo {
            id: "tron".into(),
            name: "TRON".into(),
            family: NetworkFamily::Tron,
            chain_id: None,
            native_asset: "TRX".into(),
            explorer_url: Some("https://tronscan.org/".into()),
            history_sync_available: false,
            history_coverage: Some(vec!["native-trx", "trc20"]),
            api_base_url: None,
        },
        NetworkInfo {
            id: "xrpl".into(),
            name: "XRP Ledger".into(),
            family: NetworkFamily::Xrpl,
            chain_id: None,
            native_asset: "XRP".into(),
            explorer_url: Some("https://livenet.xrpl.org/".into()),
            history_sync_available: true,
            history_coverage: Some(vec!["xrp-direct-payment"]),
            api_base_url: None,
        },
        NetworkInfo {
            id: "ton".into(),
            name: "TON".into(),
            family: NetworkFamily::Ton,
            chain_id: None,
            native_asset: "TON".into(),
            explorer_url: Some("https://tonviewer.com/".into()),
            history_sync_available: false,
            history_coverage: Some(vec!["ton-native", "ton-jetton"]),
            api_base_url: None,
        },
    ];
    networks.sort_by_key(|network| network.name.to_lowercase());
    networks
}

fn static_evm(id: &str, name: &str, chain_id: u64, asset: &str, url: &str) -> NetworkInfo {
    let api_base_url = valid_public_https_base_url(url);
    NetworkInfo {
        id: id.into(),
        name: name.into(),
        family: NetworkFamily::Evm,
        chain_id: Some(chain_id),
        native_asset: asset.into(),
        explorer_url: Some(url.into()),
        history_sync_available: api_base_url.is_some(),
        history_coverage: None,
        api_base_url,
    }
}

fn legacy_id(chain_id: u64) -> Option<&'static str> {
    match chain_id {
        1 => Some("ethereum"),
        8453 => Some("base"),
        42161 => Some("arbitrum"),
        _ => None,
    }
}

fn merge_legacy_fallbacks(networks: &mut Vec<NetworkInfo>) {
    let fallbacks = fallback_catalog();
    for fallback in fallbacks {
        if !networks.iter().any(|network| {
            network.id == fallback.id
                || fallback
                    .chain_id
                    .is_some_and(|chain_id| network.chain_id == Some(chain_id))
        }) {
            networks.push(fallback);
        }
    }
}

fn valid_public_https_base_url(value: &str) -> Option<Url> {
    let url = Url::parse(value.trim()).ok()?;
    let host = url.host_str()?;
    let host_is_public_name = !host.eq_ignore_ascii_case("localhost")
        && !host.ends_with(".localhost")
        && !host.ends_with(".local")
        && !host.ends_with(".internal")
        && host.contains('.');
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
        || url.port_or_known_default() != Some(443)
        || IpAddr::from_str(host).is_ok()
        || !host_is_public_name
    {
        return None;
    }
    Some(url)
}

pub(crate) async fn list(
    State(state): State<ApiState>,
) -> Result<Json<NetworkCatalogResponse>, ApiError> {
    let mut networks = state.blockscout.network_catalog().await;
    if let Some(solana) = networks.iter_mut().find(|network| network.id == "solana") {
        solana.history_sync_available =
            crate::solana_sync::rpc_is_configured(&state.solana_rpc_url);
    }
    if let Some(tron) = networks.iter_mut().find(|network| network.id == "tron") {
        tron.history_sync_available = state.tron_grid.is_configured();
    }
    if let Some(ton) = networks.iter_mut().find(|network| network.id == "ton") {
        ton.history_sync_available = state.ton_center.is_configured();
    }
    let history_sync_available = networks
        .iter()
        .any(|network| network.history_sync_available);
    Ok(Json(NetworkCatalogResponse {
        history_sync_available,
        networks,
    }))
}

#[cfg(test)]
mod tests {
    use super::{
        NetworkFamily, fallback_catalog, parse_blockscout_config, valid_public_https_base_url,
    };

    #[test]
    fn parses_only_mainnet_metadata_and_uses_stable_ids_for_legacy_chains() {
        let config = br#"{
          "chains": {"1":"https://eth.blockscout.com","10":"https://explorer.optimism.io","11155111":"https://sepolia.example.org"},
          "chains_metadata": {
            "1":{"name":"Ethereum","isTestnet":false,"native_currency":"ETH","instance_url_from_config":"https://eth.blockscout.com","explorers":[{"url":"https://eth.blockscout.com/"}]},
            "10":{"name":"OP Mainnet","isTestnet":false,"native_currency":"ETH","instance_url_from_config":"https://explorer.optimism.io","explorers":[{"url":"https://explorer.optimism.io/"}]},
            "11155111":{"name":"Sepolia","isTestnet":true,"native_currency":"ETH"}
          }
        }"#;

        let networks = parse_blockscout_config(config, false).unwrap();
        assert_eq!(networks.len(), 8); // Каталог включает шесть fallback-сетей.
        let ethereum = networks
            .iter()
            .find(|network| network.chain_id == Some(1))
            .unwrap();
        let optimism = networks
            .iter()
            .find(|network| network.chain_id == Some(10))
            .unwrap();
        assert_eq!(ethereum.id, "ethereum");
        assert_eq!(optimism.id, "eip155:10");
        assert_eq!(optimism.family, NetworkFamily::Evm);
        assert!(networks.iter().all(|network| network.name != "Sepolia"));
        assert!(optimism.history_sync_available);
    }

    #[test]
    fn non_blockscout_explorer_is_not_advertised_as_a_direct_sync_provider() {
        let config = br#"{
          "chains": {"55":"https://explorer.example.org"},
          "chains_metadata": {
            "55":{"name":"Example Chain","isTestnet":false,"native_currency":"EX","explorers":[{"url":"https://explorer.example.org/","hostedBy":"other"}]}
          }
        }"#;
        let networks = parse_blockscout_config(config, false).unwrap();
        let chain = networks
            .iter()
            .find(|network| network.chain_id == Some(55))
            .unwrap();
        assert!(!chain.history_sync_available);
        assert!(chain.api_base_url.is_none());

        let with_universal_provider = parse_blockscout_config(config, true).unwrap();
        let chain = with_universal_provider
            .iter()
            .find(|network| network.chain_id == Some(55))
            .unwrap();
        assert!(chain.history_sync_available);
    }

    #[test]
    fn rejects_untrusted_explorer_urls_and_oversized_or_invalid_catalogs() {
        for value in [
            "http://explorer.example.org",
            "https://localhost",
            "https://127.0.0.1",
            "https://example.org/path",
            "https://user:secret@example.org",
            "https://explorer.example.org/?redirect=1",
        ] {
            assert!(valid_public_https_base_url(value).is_none(), "{value}");
        }
        assert!(parse_blockscout_config(&vec![b' '; 2 * 1024 * 1024 + 1], false).is_err());
        assert!(parse_blockscout_config(br#"{"chains":{},"chains_metadata":{}}"#, false).is_err());
    }

    #[test]
    fn fallback_keeps_existing_networks_and_bitcoin_sync_available() {
        let fallback = fallback_catalog();
        assert!(
            fallback
                .iter()
                .any(|network| network.id == "bitcoin" && network.history_sync_available)
        );
        assert!(fallback.iter().any(|network| network.id == "arbitrum"));
        assert!(fallback.iter().any(|network| {
            network.id == "solana"
                && network.family == NetworkFamily::Solana
                && network.history_sync_available
        }));
        assert!(fallback.iter().any(|network| {
            network.id == "xrpl"
                && network.family == NetworkFamily::Xrpl
                && network.chain_id.is_none()
        }));
    }

    #[tokio::test]
    async fn endpoint_serves_dynamic_mainnets_and_caches_provider_config() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        use axum::{Router, body::Body, http::Request, routing::get};
        use reqwest::{Client, redirect::Policy};
        use tower::ServiceExt;

        const CONFIG: &str = r#"{
          "chains": {"10":"https://explorer.optimism.io"},
          "chains_metadata": {
            "10":{"name":"OP Mainnet","isTestnet":false,"native_currency":"ETH","instance_url_from_config":"https://explorer.optimism.io","explorers":[{"url":"https://explorer.optimism.io/","hostedBy":"blockscout"}]}
          }
        }"#;

        let request_count = Arc::new(AtomicUsize::new(0));
        let handler_request_count = request_count.clone();
        let mock = Router::new().route(
            "/api/json/config",
            get(move || {
                let request_count = handler_request_count.clone();
                async move {
                    request_count.fetch_add(1, Ordering::SeqCst);
                    CONFIG
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, mock).await;
        });
        let client = Client::builder().redirect(Policy::none()).build().unwrap();
        let provider =
            crate::evm_sync::BlockscoutClient::new(&format!("http://{address}"), None, client)
                .unwrap();
        let mut state = crate::health::unavailable_state();
        state.blockscout = provider;
        let app = crate::app(state);

        let first = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/networks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(first.into_body(), 16 * 1024)
            .await
            .unwrap();
        let catalog: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let optimism = catalog["networks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|network| network["id"] == "eip155:10")
            .unwrap();
        assert_eq!(optimism["chain_id"], 10);
        assert_eq!(optimism["name"], "OP Mainnet");
        assert_eq!(optimism["family"], "evm");
        assert_eq!(optimism["history_sync_available"], true);

        let second = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/networks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second.status(), axum::http::StatusCode::OK);
        assert_eq!(request_count.load(Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    async fn network_endpoint_returns_a_static_catalog_when_provider_is_unavailable() {
        use axum::{body::to_bytes, http::Request};
        use tower::ServiceExt;

        let mut state = crate::health::unavailable_state();
        state.solana_rpc_url = "http://rpc.example.org".into();
        let response = crate::app(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/networks")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
        let catalog: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(catalog["history_sync_available"], true);
        let networks = catalog["networks"].as_array().unwrap();
        assert!(networks.iter().any(|network| network["id"] == "bitcoin"));
        assert!(networks.iter().any(|network| network["id"] == "ethereum"));
        assert!(networks.iter().any(|network| network["id"] == "arbitrum"));
        assert!(networks.iter().any(|network| {
            network["id"] == "solana"
                && network["family"] == "solana"
                && network["chain_id"].is_null()
                && network["history_sync_available"] == false
        }));
    }
}
