use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::models::log_row::LogRow;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LogEvent {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub service: String,
    pub message: String,
    pub attributes: Map<String, Value>,
}

impl LogEvent {
    pub fn new(
        timestamp: DateTime<Utc>,
        level: LogLevel,
        service: &str,
        message: &str,
        attributes: Map<String, Value>,
    ) -> LogEvent {
        LogEvent {
            timestamp,
            level,
            service: service.to_string(),
            message: message.to_string(),
            attributes,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LogRowConversionError {
    #[error("invalid timestamp in milliseconds: {0}")]
    Timestamp(i64),
    #[error(transparent)]
    Level(#[from] InvalidLogLevel),
    #[error("invalid stored log attributes")]
    Attributes(#[from] serde_json::Error),
}

impl TryFrom<&LogRow> for LogEvent {
    type Error = LogRowConversionError;

    fn try_from(row: &LogRow) -> Result<Self, Self::Error> {
        let timestamp = DateTime::from_timestamp_millis(row.timestamp)
            .ok_or(LogRowConversionError::Timestamp(row.timestamp))?;

        let level = LogLevel::try_from(row.level)?;

        Ok(LogEvent::new(
            timestamp,
            level,
            &row.service,
            &row.message,
            serde_json::from_str(&row.attributes)?,
        ))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<LogLevel> for i8 {
    fn from(event: LogLevel) -> Self {
        match event {
            LogLevel::Trace => 0,
            LogLevel::Debug => 1,
            LogLevel::Info => 2,
            LogLevel::Warn => 3,
            LogLevel::Error => 4,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid log level: {0}")]
pub struct InvalidLogLevel(pub i8);

impl TryFrom<i8> for LogLevel {
    type Error = InvalidLogLevel;

    fn try_from(level: i8) -> Result<Self, InvalidLogLevel> {
        match level {
            0 => Ok(LogLevel::Trace),
            1 => Ok(LogLevel::Debug),
            2 => Ok(LogLevel::Info),
            3 => Ok(LogLevel::Warn),
            4 => Ok(LogLevel::Error),
            _ => Err(InvalidLogLevel(level)),
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct LogQuery {
    pub level: Option<LogLevel>,
    pub service: Option<String>,
    pub limit: Option<usize>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_levels_match_json_and_clickhouse_values() {
        for (level, number, name) in [
            (LogLevel::Trace, 0, "trace"),
            (LogLevel::Debug, 1, "debug"),
            (LogLevel::Info, 2, "info"),
            (LogLevel::Warn, 3, "warn"),
            (LogLevel::Error, 4, "error"),
        ] {
            assert_eq!(i8::from(level), number);
            assert_eq!(LogLevel::try_from(number).unwrap(), level);
            let json = serde_json::json!(name);
            assert_eq!(serde_json::to_value(level).unwrap(), json);
            assert_eq!(serde_json::from_value::<LogLevel>(json).unwrap(), level);
        }
        for number in [-1, 5, i8::MAX] {
            assert!(
                matches!(LogLevel::try_from(number), Err(InvalidLogLevel(value)) if value == number)
            );
        }
        for name in ["INFO", "critical"] {
            assert!(serde_json::from_value::<LogLevel>(serde_json::json!(name)).is_err());
        }
    }

    #[test]
    fn query_parses_optional_filters_and_rfc3339_offsets() {
        let uri = "/v1/logs".parse().unwrap();
        let axum::extract::Query(query) =
            axum::extract::Query::<LogQuery>::try_from_uri(&uri).unwrap();
        assert!(query.service.is_none() && query.level.is_none() && query.limit.is_none());
        assert!(query.from.is_none() && query.to.is_none());

        let uri = "/v1/logs?service=shop%20one&level=warn&limit=7&from=2026-10-07T10:00:00.123%2B02:00&to=2026-10-07T08:01:00.123Z".parse().unwrap();
        let axum::extract::Query(query) =
            axum::extract::Query::<LogQuery>::try_from_uri(&uri).unwrap();
        assert_eq!(query.service.as_deref(), Some("shop one"));
        assert_eq!(query.level, Some(LogLevel::Warn));
        assert_eq!(query.limit, Some(7));
        assert_eq!(
            query.from,
            Some("2026-10-07T08:00:00.123Z".parse().unwrap())
        );
        assert_eq!(query.to, Some("2026-10-07T08:01:00.123Z".parse().unwrap()));
        for query in [
            "level=critical",
            "limit=-1",
            "limit=many",
            "from=invalid",
            "to=invalid",
        ] {
            let uri = format!("/v1/logs?{query}").parse().unwrap();
            assert!(axum::extract::Query::<LogQuery>::try_from_uri(&uri).is_err());
        }
    }

    #[test]
    fn converts_log_rows_and_rejects_invalid_values() {
        let timestamp = "2026-10-07T08:00:00.123Z".parse().unwrap();
        for (level, number) in [
            (LogLevel::Trace, 0),
            (LogLevel::Debug, 1),
            (LogLevel::Info, 2),
            (LogLevel::Warn, 3),
            (LogLevel::Error, 4),
        ] {
            let attributes = serde_json::json!({"printer": "receipt", "retries": 2})
                .as_object()
                .unwrap()
                .clone();
            let event = LogEvent::new(timestamp, level, "service-test", "Test", attributes);
            let row = LogRow::from(event.clone());
            assert_eq!(row.timestamp, 1_791_360_000_123);
            assert_eq!(row.level, number);
            assert_eq!(LogEvent::try_from(&row).unwrap(), event);
        }

        let mut row = LogRow::from(LogEvent::new(
            timestamp,
            LogLevel::Debug,
            "service-test",
            "Test",
            Map::new(),
        ));
        row.level = 5;
        assert!(matches!(
            LogEvent::try_from(&row),
            Err(LogRowConversionError::Level(InvalidLogLevel(5)))
        ));

        row.level = 1;
        row.timestamp = i64::MAX;
        assert!(matches!(
            LogEvent::try_from(&row),
            Err(LogRowConversionError::Timestamp(i64::MAX))
        ));
        row.timestamp = 1_791_360_000_123;
        row.attributes = "[]".into();
        assert!(matches!(
            LogEvent::try_from(&row),
            Err(LogRowConversionError::Attributes(_))
        ));
    }
}
