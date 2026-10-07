use clickhouse::Row;
use serde::{Deserialize, Serialize};

use super::log_event::{LogEvent, LogLevel};

#[derive(Row, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct LogRow {
    pub timestamp: i64,
    pub level: i8,
    pub service: String,
    pub message: String,
    pub attributes: String,
}

impl From<LogEvent> for LogRow {
    fn from(event: LogEvent) -> Self {
        Self {
            timestamp: event.timestamp.timestamp_millis(),
            level: match event.level {
                LogLevel::Trace => 0,
                LogLevel::Debug => 1,
                LogLevel::Info => 2,
                LogLevel::Warn => 3,
                LogLevel::Error => 4,
            },
            service: event.service,
            message: event.message,
            attributes: serde_json::to_string(&event.attributes)
                .expect("JSON attributes must serialize"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use clickhouse::{Client, sql::Identifier};

    #[tokio::test]
    #[ignore = "requires local ClickHouse and exported CLICKHOUSE_DB, CLICKHOUSE_USER, CLICKHOUSE_PASSWORD"]
    async fn clickhouse_log_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let client = Client::default()
            // test-util disables compression by default; exercise the application's LZ4 path.
            .with_compression(clickhouse::Compression::Lz4)
            .with_setting("network_compression_method", "lz4")
            .with_url(
                std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://127.0.0.1:8123".into()),
            )
            .with_database(std::env::var("CLICKHOUSE_DB")?)
            .with_user(std::env::var("CLICKHOUSE_USER")?)
            .with_password(std::env::var("CLICKHOUSE_PASSWORD")?);
        let table = format!("_log_row_test_{}", std::process::id());
        client
            .query("CREATE TABLE ? (timestamp DateTime64(3, 'UTC'), level Enum8('trace' = 0, 'debug' = 1, 'info' = 2, 'warn' = 3, 'error' = 4), service String, message String, attributes String) ENGINE = MergeTree ORDER BY timestamp")
            .bind(Identifier(&table))
            .execute()
            .await?;

        let result: clickhouse::error::Result<_> = async {
            let mut expected = Vec::new();
            let mut insert = client.insert::<LogRow>(&table).await?;
            for level in [
                LogLevel::Trace,
                LogLevel::Debug,
                LogLevel::Info,
                LogLevel::Warn,
                LogLevel::Error,
            ] {
                let row = LogRow::from(LogEvent::new(
                    Utc::now(),
                    level,
                    "service-test",
                    "Test",
                    serde_json::json!({"printer": "receipt", "retries": 2})
                        .as_object()
                        .unwrap()
                        .clone(),
                ));
                insert.write(&row).await?;
                expected.push(row);
            }
            insert.end().await?;
            let actual = client
                .query("SELECT ?fields FROM ? ORDER BY level")
                .bind(Identifier(&table))
                .fetch_all::<LogRow>()
                .await?;
            Ok((expected, actual))
        }
        .await;

        client
            .query("DROP TABLE ? SYNC")
            .bind(Identifier(&table))
            .execute()
            .await?;
        let (expected, actual) = result?;
        assert_eq!(actual, expected);
        Ok(())
    }
}
