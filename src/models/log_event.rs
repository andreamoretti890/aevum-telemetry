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
    InvalidTimestamp(i64),
    #[error(transparent)]
    InvalidLevel(#[from] InvalidLogLevel),
}

impl TryFrom<&LogRow> for LogEvent {
    type Error = LogRowConversionError;

    fn try_from(row: &LogRow) -> Result<Self, Self::Error> {
        let timestamp = DateTime::from_timestamp_millis(row.timestamp)
            .ok_or(LogRowConversionError::InvalidTimestamp(row.timestamp))?;

        let level = LogLevel::try_from(row.level)?;

        Ok(LogEvent::new(
            timestamp,
            level,
            &row.service,
            &row.message,
            serde_json::from_str(&row.attributes).unwrap_or_default(),
        ))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
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

#[derive(Deserialize)]
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
    fn converts_log_rows_and_rejects_invalid_values() {
        let timestamp = "2026-10-07T08:00:00.123Z".parse().unwrap();
        for level in [
            LogLevel::Trace,
            LogLevel::Debug,
            LogLevel::Info,
            LogLevel::Warn,
            LogLevel::Error,
        ] {
            let attributes = serde_json::json!({"printer": "receipt", "retries": 2})
                .as_object()
                .unwrap()
                .clone();
            let event = LogEvent::new(timestamp, level, "service-test", "Test", attributes);
            let row = LogRow::from(event.clone());
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
            Err(LogRowConversionError::InvalidLevel(InvalidLogLevel(5)))
        ));

        row.level = 1;
        row.timestamp = i64::MAX;
        assert!(matches!(
            LogEvent::try_from(&row),
            Err(LogRowConversionError::InvalidTimestamp(i64::MAX))
        ));
    }
}
