use std::{env, error::Error, path::PathBuf, time::Duration};

use sqlx::postgres::PgPoolOptions;
use tracefold_api::sanctions::{import_ofac_snapshot, parse_ofac_sdn_xml};

const OFAC_SDN_URL: &str =
    "https://sanctionslistservice.ofac.treas.gov/api/PublicationPreview/exports/SDN.XML";
const MAX_DOWNLOAD_BYTES: u64 = 50 * 1024 * 1024;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = dotenvy::dotenv();
    let database_url = env::var("DATABASE_URL")?;
    let input_path = env::args_os().nth(1).map(PathBuf::from);
    let xml = match input_path {
        Some(path) => tokio::fs::read(path).await?,
        None => download_current_snapshot().await?,
    };
    if xml.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err("OFAC SDN XML exceeds the 50 MiB limit".into());
    }
    let xml = std::str::from_utf8(&xml)?;
    let snapshot = parse_ofac_sdn_xml(xml)?;
    let database = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&database).await?;
    let imported = import_ofac_snapshot(&database, snapshot).await?;
    println!("Imported {imported} OFAC SDN digital currency addresses.");
    Ok(())
}

async fn download_current_snapshot() -> Result<Vec<u8>, Box<dyn Error>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .user_agent("Tracefold/0.1 (+https://github.com/aferapokitaysky/blockchain_view)")
        .build()?;
    let response = client.get(OFAC_SDN_URL).send().await?.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DOWNLOAD_BYTES)
    {
        return Err("OFAC SDN XML exceeds the 50 MiB limit".into());
    }
    Ok(response.bytes().await?.to_vec())
}
