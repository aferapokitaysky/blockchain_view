use std::{env, net::SocketAddr, str::FromStr};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("не задана обязательная переменная {0}")]
    Missing(&'static str),
    #[error("некорректный адрес прослушивания BIND_ADDR")]
    InvalidBindAddress,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub bind_address: SocketAddr,
    pub database_url: String,
    pub redis_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind_address = env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
        let database_url =
            env::var("DATABASE_URL").map_err(|_| ConfigError::Missing("DATABASE_URL"))?;
        let redis_url = env::var("REDIS_URL").map_err(|_| ConfigError::Missing("REDIS_URL"))?;

        Self::from_values(&bind_address, database_url, redis_url)
    }

    fn from_values(
        bind_address: &str,
        database_url: String,
        redis_url: String,
    ) -> Result<Self, ConfigError> {
        let bind_address =
            SocketAddr::from_str(bind_address).map_err(|_| ConfigError::InvalidBindAddress)?;
        Ok(Self {
            bind_address,
            database_url,
            redis_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, ConfigError};

    #[test]
    fn parses_bind_address_and_dependency_urls() {
        let config = Config::from_values(
            "127.0.0.1:9000",
            "postgres://tracefold:tracefold@localhost/tracefold".into(),
            "redis://localhost:6379".into(),
        )
        .expect("валидная конфигурация должна разбираться");

        assert_eq!(config.bind_address.port(), 9000);
        assert_eq!(config.redis_url, "redis://localhost:6379");
    }

    #[test]
    fn rejects_invalid_bind_address() {
        let result = Config::from_values("invalid", "postgres://db".into(), "redis://cache".into());

        assert!(matches!(result, Err(ConfigError::InvalidBindAddress)));
    }
}
