use axum::{
    Json,
    extract::{Query, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};

use crate::{
    ApiState,
    addresses::{Network, normalize_evm_address},
    error::ApiError,
    networks::NetworkFamily,
};

const SOURCE_CODE: &str = "ofac_sdn";
const SOURCE_NAME: &str = "OFAC Specially Designated Nationals List";
const MAX_XML_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum SanctionsImportError {
    #[error("OFAC SDN XML exceeds the configured size limit")]
    SnapshotTooLarge,
    #[error("OFAC SDN XML is malformed or has an unsupported structure")]
    InvalidXml(#[from] quick_xml::DeError),
    #[error("OFAC SDN XML does not contain usable digital currency addresses")]
    NoAddresses,
}

#[derive(Debug)]
pub struct ParsedOfacSnapshot {
    published_at: String,
    addresses: Vec<ParsedSanctionsAddress>,
}

#[derive(Debug)]
struct ParsedSanctionsAddress {
    entity_uid: String,
    entity_name: String,
    programs: Vec<String>,
    currency_code: String,
    address: String,
    address_normalized: String,
}

#[derive(Debug, Deserialize)]
struct OfacSdnList {
    #[serde(rename = "publshInformation")]
    publish_information: PublishInformation,
    #[serde(rename = "sdnEntry", default)]
    entries: Vec<SdnEntry>,
}

#[derive(Debug, Deserialize)]
struct PublishInformation {
    #[serde(rename = "Publish_Date")]
    publish_date: String,
}

#[derive(Debug, Deserialize)]
struct SdnEntry {
    uid: String,
    #[serde(rename = "firstName", default)]
    first_name: Option<String>,
    #[serde(rename = "lastName", default)]
    last_name: Option<String>,
    #[serde(rename = "programList", default)]
    program_list: Option<ProgramList>,
    #[serde(rename = "idList", default)]
    id_list: Option<IdList>,
}

#[derive(Debug, Deserialize)]
struct ProgramList {
    #[serde(rename = "program", default)]
    programs: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct IdList {
    #[serde(rename = "id", default)]
    ids: Vec<SdnIdentifier>,
}

#[derive(Debug, Deserialize)]
struct SdnIdentifier {
    #[serde(rename = "idType", default)]
    id_type: Option<String>,
    #[serde(rename = "idNumber", default)]
    id_number: Option<String>,
}

pub fn parse_ofac_sdn_xml(xml: &str) -> Result<ParsedOfacSnapshot, SanctionsImportError> {
    if xml.len() > MAX_XML_BYTES {
        return Err(SanctionsImportError::SnapshotTooLarge);
    }
    let parsed: OfacSdnList = quick_xml::de::from_str(xml)?;
    let mut addresses = Vec::new();
    for entry in parsed.entries {
        let entity_uid = entry.uid.trim();
        if entity_uid.is_empty() {
            continue;
        }
        let entity_name = [entry.first_name.as_deref(), entry.last_name.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if entity_name.is_empty() {
            continue;
        }
        let programs = entry
            .program_list
            .map(|list| {
                list.programs
                    .into_iter()
                    .map(|program| program.trim().to_owned())
                    .filter(|program| !program.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for identifier in entry.id_list.into_iter().flat_map(|list| list.ids) {
            let Some(id_type) = identifier.id_type.as_deref().map(str::trim) else {
                continue;
            };
            let Some(currency_code) = id_type
                .strip_prefix("Digital Currency Address -")
                .map(str::trim)
                .filter(|value| !value.is_empty() && value.len() <= 16)
            else {
                continue;
            };
            let Some(address) = identifier
                .id_number
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty() && value.len() <= 256)
            else {
                continue;
            };
            let currency_code = currency_code.to_ascii_uppercase();
            let address_normalized = if currency_code == "ETH" {
                address.to_ascii_lowercase()
            } else {
                address.to_owned()
            };
            addresses.push(ParsedSanctionsAddress {
                entity_uid: entity_uid.to_owned(),
                entity_name: entity_name.clone(),
                programs: programs.clone(),
                currency_code,
                address: address.to_owned(),
                address_normalized,
            });
        }
    }
    if addresses.is_empty() {
        return Err(SanctionsImportError::NoAddresses);
    }
    let published_at = parsed.publish_information.publish_date.trim().to_owned();
    if published_at.is_empty() {
        return Err(SanctionsImportError::NoAddresses);
    }
    Ok(ParsedOfacSnapshot {
        published_at,
        addresses,
    })
}

pub async fn import_ofac_snapshot(
    database: &PgPool,
    snapshot: ParsedOfacSnapshot,
) -> Result<usize, sqlx::Error> {
    let mut transaction = database.begin().await?;
    sqlx::query("DELETE FROM sanctions_list_snapshots WHERE source_code = $1")
        .bind(SOURCE_CODE)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "INSERT INTO sanctions_list_snapshots (source_code, source_name, published_at, address_count) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(SOURCE_CODE)
    .bind(SOURCE_NAME)
    .bind(snapshot.published_at)
    .bind(snapshot.addresses.len() as i32)
    .execute(&mut *transaction)
    .await?;

    for address in &snapshot.addresses {
        sqlx::query(
            "INSERT INTO sanctions_addresses \
             (source_code, entity_uid, entity_name, programs, currency_code, address, address_normalized) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(SOURCE_CODE)
        .bind(&address.entity_uid)
        .bind(&address.entity_name)
        .bind(serde_json::Value::Array(
            address
                .programs
                .iter()
                .cloned()
                .map(serde_json::Value::String)
                .collect(),
        ))
        .bind(&address.currency_code)
        .bind(&address.address)
        .bind(&address.address_normalized)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(snapshot.addresses.len())
}

#[derive(Debug, Deserialize)]
pub struct ScreenAddressQuery {
    network: String,
    address: String,
}

#[derive(Debug, FromRow, Serialize)]
struct DatasetSnapshot {
    source_name: String,
    published_at: String,
    imported_at: DateTime<Utc>,
    address_count: i32,
}

#[derive(Debug, FromRow, Serialize)]
struct ExactAddressMatch {
    entity_uid: String,
    entity_name: String,
    programs: serde_json::Value,
    currency_code: String,
    address: String,
}

#[derive(Debug, Serialize)]
pub struct ScreenAddressResponse {
    network: String,
    status: &'static str,
    match_type: &'static str,
    source: Option<DatasetSnapshot>,
    matches: Vec<ExactAddressMatch>,
    explanation: &'static str,
}

pub async fn screen_address(
    State(state): State<ApiState>,
    Query(query): Query<ScreenAddressQuery>,
) -> Result<Json<ScreenAddressResponse>, ApiError> {
    let catalog_entry = crate::networks::provider_catalog(&state)
        .await
        .into_iter()
        .find(|network| network.id == query.network)
        .ok_or(ApiError::UnsupportedNetwork)?;
    let network = catalog_entry.id.clone();
    let (address, currency) = match network.as_str() {
        "bitcoin" => (Network::Bitcoin.normalize(&query.address)?, Some("XBT")),
        "ethereum" => (normalize_evm_address(&query.address)?, Some("ETH")),
        "tron" => (Network::Tron.normalize(&query.address)?, Some("TRX")),
        "xrpl" => (Network::Xrpl.normalize(&query.address)?, Some("XRP")),
        "base" | "arbitrum" => (normalize_evm_address(&query.address)?, None),
        _ if catalog_entry.family == NetworkFamily::Evm => {
            (normalize_evm_address(&query.address)?, None)
        }
        _ => return Err(ApiError::UnsupportedNetwork),
    };
    let Some(currency) = currency else {
        return Ok(Json(ScreenAddressResponse {
            network: network.clone(),
            status: "network_not_covered",
            match_type: "exact_address",
            source: None,
            matches: Vec::new(),
            explanation: "OFAC currency identifiers are mapped only for Bitcoin (XBT), Ethereum (ETH), TRON (TRX), and XRP Ledger (XRP); no conclusion is made for this network.",
        }));
    };

    let source = sqlx::query_as::<_, DatasetSnapshot>(
        "SELECT source_name, published_at, imported_at, address_count \
         FROM sanctions_list_snapshots WHERE source_code = $1",
    )
    .bind(SOURCE_CODE)
    .fetch_optional(&state.database)
    .await?;
    let Some(source) = source else {
        return Ok(Json(ScreenAddressResponse {
            network: network.clone(),
            status: "source_not_imported",
            match_type: "exact_address",
            source: None,
            matches: Vec::new(),
            explanation: "The OFAC SDN snapshot has not been imported locally; this is not a negative screening result.",
        }));
    };

    let matches = sqlx::query_as::<_, ExactAddressMatch>(
        "SELECT entity_uid, entity_name, programs, currency_code, address \
         FROM sanctions_addresses \
         WHERE source_code = $1 AND currency_code = $2 AND address_normalized = $3 \
         ORDER BY entity_uid",
    )
    .bind(SOURCE_CODE)
    .bind(currency)
    .bind(if currency == "ETH" {
        address.to_ascii_lowercase()
    } else {
        address
    })
    .fetch_all(&state.database)
    .await?;
    let status = if matches.is_empty() {
        "no_exact_match"
    } else {
        "exact_match"
    };
    Ok(Json(ScreenAddressResponse {
        network: network.clone(),
        status,
        match_type: "exact_address",
        source: Some(source),
        matches,
        explanation: "An exact address entry in this provider snapshot is a review signal, not an independent legal or identity conclusion. A non-match covers only this source, currency, and imported snapshot.",
    }))
}

#[cfg(test)]
mod tests {
    use super::{SanctionsImportError, parse_ofac_sdn_xml};

    const SDN_FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<sdnList>
  <publshInformation><Publish_Date>11/12/2026 01:00:00 PM</Publish_Date><Record_Count>1</Record_Count></publshInformation>
  <sdnEntry>
    <uid>12345</uid><lastName>Example Exchange</lastName><sdnType>Entity</sdnType>
    <programList><program>TEST-PROGRAM</program></programList>
    <idList>
      <id><uid>1</uid><idType>Digital Currency Address - ETH</idType><idNumber>0xAbCd000000000000000000000000000000000001</idNumber><idCountry/></id>
      <id><uid>2</uid><idType>Digital Currency Address - XBT</idType><idNumber>bc1qexample</idNumber><idCountry/></id>
      <id><uid>3</uid><idType>Digital Currency Address - TRX</idType><idNumber>TUoHaVjx7n5xz8LwPRDckgFrDWhMhuSuJM</idNumber><idCountry/></id>
      <id><uid>4</uid><idType>Digital Currency Address - XRP</idType><idNumber>rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh</idNumber><idCountry/></id>
      <id><uid>5</uid><idType>Passport</idType><idNumber>PUBLIC-IDENTIFIER</idNumber><idCountry>US</idCountry></id>
    </idList>
  </sdnEntry>
</sdnList>"#;

    #[test]
    fn parses_exact_currency_address_records_and_metadata() {
        let snapshot = parse_ofac_sdn_xml(SDN_FIXTURE).expect("fixture should parse");

        assert_eq!(snapshot.published_at, "11/12/2026 01:00:00 PM");
        assert_eq!(snapshot.addresses.len(), 4);
        assert_eq!(snapshot.addresses[0].entity_uid, "12345");
        assert_eq!(snapshot.addresses[0].entity_name, "Example Exchange");
        assert_eq!(snapshot.addresses[0].programs, ["TEST-PROGRAM"]);
        assert_eq!(snapshot.addresses[0].currency_code, "ETH");
        assert_eq!(
            snapshot.addresses[0].address_normalized,
            "0xabcd000000000000000000000000000000000001"
        );
        assert_eq!(snapshot.addresses[1].currency_code, "XBT");
        assert_eq!(snapshot.addresses[1].address_normalized, "bc1qexample");
        assert_eq!(snapshot.addresses[2].currency_code, "TRX");
        assert_eq!(
            snapshot.addresses[2].address_normalized,
            "TUoHaVjx7n5xz8LwPRDckgFrDWhMhuSuJM"
        );
        assert_eq!(snapshot.addresses[3].currency_code, "XRP");
        assert_eq!(
            snapshot.addresses[3].address_normalized,
            "rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh"
        );
    }

    #[test]
    fn rejects_a_valid_xml_document_without_digital_currency_addresses() {
        let xml = SDN_FIXTURE
            .replace("Digital Currency Address - ETH", "Passport")
            .replace("Digital Currency Address - XBT", "Passport")
            .replace("Digital Currency Address - TRX", "Passport")
            .replace("Digital Currency Address - XRP", "Passport");
        assert!(matches!(
            parse_ofac_sdn_xml(&xml),
            Err(SanctionsImportError::NoAddresses)
        ));
    }
}
