use std::error::Error;

use axum::{
    Json,
    extract::rejection::{JsonRejection, QueryRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use uuid::Uuid;

use crate::models::log_event::LogRowConversionError;

pub const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_BATCH_EVENTS: usize = 1000;

/// Keeps internal causes separate from the public API error message.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("failed to {operation}")]
    Storage {
        operation: &'static str,
        #[source]
        source: clickhouse::error::Error,
    },
    #[error("a stored log could not be decoded")]
    StoredLogInvalid(#[from] LogRowConversionError),
    #[error("{message}")]
    InvalidRequest {
        message: &'static str,
        field: Option<&'static str>,
        event_index: Option<usize>,
    },
    #[error("batch exceeds the maximum of {limit} events")]
    BatchTooLarge { limit: usize },
    #[error("invalid JSON request")]
    Json(#[from] JsonRejection),
    #[error("invalid query parameters")]
    Query(#[from] QueryRejection),
    #[error("route not found")]
    NotFound,
    #[error("method not allowed")]
    MethodNotAllowed,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<usize>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::Storage { .. } => (StatusCode::INTERNAL_SERVER_ERROR, "storage_error"),
            Self::StoredLogInvalid(_) => (StatusCode::INTERNAL_SERVER_ERROR, "stored_log_invalid"),
            Self::InvalidRequest { .. } => (StatusCode::BAD_REQUEST, "invalid_request"),
            Self::BatchTooLarge { .. } => (StatusCode::PAYLOAD_TOO_LARGE, "batch_too_large"),
            Self::Json(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large")
            }
            Self::Json(rejection) => (rejection.status(), "invalid_request"),
            Self::Query(rejection) => (rejection.status(), "invalid_request"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::MethodNotAllowed => (StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"),
        };

        // Assign the ID where the error is handled, so the response and its one log entry agree.
        let mut error = ErrorBody {
            code,
            message: self.to_string(),
            request_id: Uuid::new_v4().to_string(),
            field: None,
            event_index: None,
            limit: None,
        };
        match &self {
            Self::InvalidRequest {
                field, event_index, ..
            } => {
                error.field = *field;
                error.event_index = *event_index;
            }
            Self::BatchTooLarge { limit } => error.limit = Some(*limit),
            Self::Json(_) if status == StatusCode::PAYLOAD_TOO_LARGE => {
                error.message =
                    format!("request body exceeds the maximum of {MAX_BODY_BYTES} bytes");
                error.limit = Some(MAX_BODY_BYTES);
            }
            Self::Json(rejection) => error.message = rejection.body_text(),
            Self::Query(rejection) => error.message = rejection.body_text(),
            _ => {}
        }

        if status.is_server_error() {
            let mut causes = Vec::new();
            let mut source = self.source();
            while let Some(cause) = source {
                causes.push(cause.to_string());
                source = cause.source();
            }
            tracing::error!(
                request_id = %error.request_id,
                code,
                status = status.as_u16(),
                error = %self,
                causes = ?causes,
                "API request failed"
            );
        } else {
            // Request bodies and rejected field values do not belong in server logs.
            tracing::warn!(
                request_id = %error.request_id,
                code,
                status = status.as_u16(),
                "API request rejected"
            );
        }

        (status, Json(ErrorResponse { error })).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);

    impl io::Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn logs_cause_once_with_the_response_request_id() {
        let buffer = LogBuffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(move || writer.clone())
            .finish();
        let response = tracing::subscriber::with_default(subscriber, || {
            ApiError::Storage {
                operation: "store log batch",
                source: clickhouse::error::Error::Network(Box::new(io::Error::other(
                    "private database detail",
                ))),
            }
            .into_response()
        });
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "storage_error");
        assert_eq!(body["error"]["message"], "failed to store log batch");
        assert!(!String::from_utf8_lossy(&bytes).contains("private database detail"));

        let logs = buffer.0.lock().unwrap();
        let entries: Vec<_> = serde_json::Deserializer::from_slice(&logs)
            .into_iter::<serde_json::Value>()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0]["fields"]["request_id"],
            body["error"]["request_id"]
        );
        assert_eq!(entries[0]["fields"]["error"], "failed to store log batch");
        assert!(
            entries[0]["fields"]["causes"]
                .as_str()
                .unwrap()
                .contains("private database detail")
        );
    }
}
