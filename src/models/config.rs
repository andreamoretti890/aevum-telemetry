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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::Ipv4Addr, process::Command};

    #[test]
    fn validates_environment() {
        // Child processes keep these cases isolated without mutating the test runner's environment.
        if let Ok(expected) = env::var("AEVUM_CONFIG_TEST_EXPECT") {
            let result = Config::from_env();
            if let Some(name) = expected.strip_prefix("missing ") {
                assert!(matches!(result, Err(ConfigError::Environment {
                    name: variable, source: env::VarError::NotPresent,
                }) if variable == name));
                return;
            }
            match expected.as_str() {
                "valid" => {
                    let config = result.unwrap();
                    assert_eq!(config.host, IpAddr::V4(Ipv4Addr::LOCALHOST));
                    assert_eq!(config.port, 3000);
                    assert_eq!(config.clickhouse_url, "http://127.0.0.1:8123");
                    assert_eq!(config.clickhouse_db, "test_database");
                    assert_eq!(config.clickhouse_table, "logs");
                    assert_eq!(config.clickhouse_user, "test_user");
                    assert_eq!(config.clickhouse_password, "test_password");
                }
                "AEVUM_HOST" => assert!(matches!(result, Err(ConfigError::InvalidHost { .. }))),
                "AEVUM_PORT" => assert!(matches!(result, Err(ConfigError::InvalidPort { .. }))),
                "CLICKHOUSE_URL" => assert!(matches!(
                    result,
                    Err(ConfigError::InvalidClickhouseUrl { .. })
                )),
                "CLICKHOUSE_DB" => assert!(matches!(
                    result,
                    Err(ConfigError::InvalidClickhouseDb { .. })
                )),
                "CLICKHOUSE_TABLE" => assert!(matches!(
                    result,
                    Err(ConfigError::InvalidClickhouseTable { .. })
                )),
                "CLICKHOUSE_USER" => assert!(matches!(
                    result,
                    Err(ConfigError::InvalidClickhouseUser { .. })
                )),
                "CLICKHOUSE_PASSWORD" => assert!(matches!(
                    result,
                    Err(ConfigError::InvalidClickhousePassword { .. })
                )),
                _ => panic!("unexpected configuration case: {expected}"),
            }
            return;
        }

        let variables = [
            ("AEVUM_HOST", "127.0.0.1"),
            ("AEVUM_PORT", "3000"),
            ("CLICKHOUSE_URL", "http://127.0.0.1:8123"),
            ("CLICKHOUSE_DB", "test_database"),
            ("CLICKHOUSE_TABLE", "logs"),
            ("CLICKHOUSE_USER", "test_user"),
            ("CLICKHOUSE_PASSWORD", "test_password"),
        ];
        let check = |override_value: Option<(&str, Option<&str>)>, expected: &str| {
            let mut command = Command::new(env::current_exe().unwrap());
            command
                .args(["--exact", "models::config::tests::validates_environment"])
                .env_clear()
                .envs(variables)
                .env("AEVUM_CONFIG_TEST_EXPECT", expected);
            if let Some((name, value)) = override_value {
                match value {
                    Some(value) => {
                        command.env(name, value);
                    }
                    None => {
                        command.env_remove(name);
                    }
                }
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "configuration case {expected}:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        };
        check(None, "valid");
        for (name, _) in variables {
            check(Some((name, None)), &format!("missing {name}"));
            check(Some((name, Some(""))), name);
        }
        for (name, value) in [
            ("AEVUM_HOST", "localhost"),
            ("AEVUM_PORT", "-1"),
            ("AEVUM_PORT", "65536"),
            ("AEVUM_PORT", "not-a-number"),
            ("CLICKHOUSE_URL", "not-a-url"),
            ("CLICKHOUSE_URL", "ftp://127.0.0.1:8123"),
            ("CLICKHOUSE_URL", "http://"),
        ] {
            check(Some((name, Some(value))), name);
        }
    }
}
