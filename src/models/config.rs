use std::env;
use std::net::IpAddr;

use thiserror::Error;
use url::Url;

#[derive(Debug)]
pub struct Config {
    pub host: IpAddr,
    pub port: u16,
    pub clickhouse_url: String,
    pub clickhouse_db: String,
    pub clickhouse_table: String,
    pub clickhouse_user: String,
    pub clickhouse_password: String,
}

impl Config {
    pub fn from_env() -> Result<Config, ConfigError> {
        let host = env::var("AEVUM_HOST").map_err(|source| ConfigError::Environment {
            name: "AEVUM_HOST",
            source,
        })?;
        if host.is_empty() {
            return Err(ConfigError::InvalidHost {
                reason: "must not be empty",
            });
        }
        let host = &host
            .parse::<IpAddr>()
            .map_err(|_| ConfigError::InvalidHost {
                reason: "must be a valid IP address",
            })?;

        let port = env::var("AEVUM_PORT").map_err(|source| ConfigError::Environment {
            name: "AEVUM_PORT",
            source,
        })?;
        if port.is_empty() {
            return Err(ConfigError::InvalidPort {
                reason: "must not be empty",
            });
        };
        let port = port.parse::<u16>().map_err(|_| ConfigError::InvalidPort {
            reason: "must be a valid port",
        })?;

        let clickhouse_url =
            env::var("CLICKHOUSE_URL").map_err(|source| ConfigError::Environment {
                name: "CLICKHOUSE_URL",
                source,
            })?;
        let url = Url::parse(&clickhouse_url).map_err(|_| ConfigError::InvalidClickhouseUrl {
            reason: "must be a valid url",
        })?;

        let scheme = url.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(ConfigError::InvalidClickhouseUrl {
                reason: "must have a valid scheme",
            });
        }
        if url.host().is_none() {
            return Err(ConfigError::InvalidClickhouseUrl {
                reason: "must have a valid host",
            });
        }

        let clickhouse_db =
            env::var("CLICKHOUSE_DB").map_err(|source| ConfigError::Environment {
                name: "CLICKHOUSE_DB",
                source,
            })?;
        if clickhouse_db.is_empty() {
            return Err(ConfigError::InvalidClickhouseDb {
                reason: "must not be empty",
            });
        }

        let clickhouse_table =
            env::var("CLICKHOUSE_TABLE").map_err(|source| ConfigError::Environment {
                name: "CLICKHOUSE_TABLE",
                source,
            })?;
        if clickhouse_table.is_empty() {
            return Err(ConfigError::InvalidClickhouseTable {
                reason: "must not be empty",
            });
        }

        let clickhouse_user =
            env::var("CLICKHOUSE_USER").map_err(|source| ConfigError::Environment {
                name: "CLICKHOUSE_USER",
                source,
            })?;
        if clickhouse_user.is_empty() {
            return Err(ConfigError::InvalidClickhouseUser {
                reason: "must not be empty",
            });
        }

        let clickhouse_password =
            env::var("CLICKHOUSE_PASSWORD").map_err(|source| ConfigError::Environment {
                name: "CLICKHOUSE_PASSWORD",
                source,
            })?;
        if clickhouse_password.is_empty() {
            return Err(ConfigError::InvalidClickhousePassword {
                reason: "must not be empty",
            });
        }

        Ok(Config {
            host: *host,
            port,
            clickhouse_url,
            clickhouse_db,
            clickhouse_table,
            clickhouse_user,
            clickhouse_password,
        })
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read {name}: {source}")]
    Environment {
        name: &'static str,
        #[source]
        source: env::VarError,
    },
    #[error("invalid AEVUM_HOST: {reason}")]
    InvalidHost { reason: &'static str },
    #[error("invalid AEVUM_PORT: {reason}")]
    InvalidPort { reason: &'static str },
    #[error("invalid CLICKHOUSE_URL: {reason}")]
    InvalidClickhouseUrl { reason: &'static str },
    #[error("invalid CLICKHOUSE_DB: {reason}")]
    InvalidClickhouseDb { reason: &'static str },
    #[error("invalid CLICKHOUSE_TABLE: {reason}")]
    InvalidClickhouseTable { reason: &'static str },
    #[error("invalid CLICKHOUSE_USER: {reason}")]
    InvalidClickhouseUser { reason: &'static str },
    #[error("invalid CLICKHOUSE_PASSWORD: {reason}")]
    InvalidClickhousePassword { reason: &'static str },
}
