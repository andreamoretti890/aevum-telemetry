use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LogEvent {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub service: String,
    pub message: String,
}

impl LogEvent {
    pub fn new(
        timestamp: DateTime<Utc>,
        level: LogLevel,
        service: &str,
        message: &str,
    ) -> LogEvent {
        LogEvent {
            timestamp,
            level,
            service: service.to_string(),
            message: message.to_string(),
        }
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
