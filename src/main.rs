mod api_error;
mod models;

use std::env;

use axum::{
    Json, Router,
    extract::rejection::{JsonRejection, QueryRejection},
    extract::{DefaultBodyLimit, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use clickhouse::{Client, sql::Identifier};
use serde_json::{Value, json};
use tokio::net::TcpListener;

use crate::api_error::{ApiError, MAX_BATCH_EVENTS, MAX_BODY_BYTES};
use crate::models::log_event::{LogEvent, LogQuery, LogRowConversionError};
use crate::models::log_row::LogRow;

#[derive(Clone, Default)]
struct AppState {
    client: Client,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .init();

    let client = Client::default()
        // Match the Rust client's LZ4 decoder; ClickHouse 26.9 defaults to ZSTD.
        .with_setting("network_compression_method", "lz4")
        .with_url(env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://127.0.0.1:8123".into()))
        .with_database(read_env_var("CLICKHOUSE_DB"))
        .with_user(read_env_var("CLICKHOUSE_USER"))
        .with_password(read_env_var("CLICKHOUSE_PASSWORD"));

    let state: AppState = AppState { client };
    let app: Router = create_app(state);
    let listener: TcpListener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;

    tracing::info!(address = "127.0.0.1:3000", "server listening");

    axum::serve(listener, app).await?;
    Ok(())
}

fn read_env_var(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("{key} env variable should be set"))
}

fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/v1/logs", get(get_handler).post(post_handler))
        .route("/v1/logs/batch", post(batch_post_handler))
        .fallback(|| async { ApiError::NotFound })
        .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
        .with_state(state)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
}

async fn health_handler() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn get_handler(
    State(state): State<AppState>,
    query: Result<Query<LogQuery>, QueryRejection>,
) -> Result<Json<Vec<LogEvent>>, ApiError> {
    let Query(query) = query?;
    if let (Some(from), Some(to)) = (query.from, query.to)
        && from > to
    {
        return Err(ApiError::InvalidRequest {
            message: "'from' must be earlier than or equal to 'to'",
            field: Some("from"),
            event_index: None,
        });
    }
    if let Some(service) = &query.service {
        validate_service(service, None)?;
    }
    let mut sql = "SELECT ?fields FROM ? WHERE 1=1".to_string();
    if query.level.is_some() {
        sql.push_str(" AND level = ?");
    }
    if query.service.is_some() {
        sql.push_str(" AND service = ?");
    }
    if query.from.is_some() && query.to.is_some() {
        sql.push_str(" AND timestamp BETWEEN fromUnixTimestamp64Milli(?, 'UTC') AND fromUnixTimestamp64Milli(?, 'UTC')");
    } else if query.from.is_some() {
        sql.push_str(" AND timestamp >= fromUnixTimestamp64Milli(?, 'UTC')");
    } else if query.to.is_some() {
        sql.push_str(" AND timestamp <= fromUnixTimestamp64Milli(?, 'UTC')");
    }

    sql.push_str(" ORDER BY timestamp DESC LIMIT ?");

    let mut q = state.client.query(&sql).bind(Identifier("logs"));
    if let Some(level) = query.level {
        q = q.bind(level);
    }
    if let Some(service) = query.service {
        q = q.bind(service);
    }

    if let Some(from) = query.from {
        q = q.bind(from.timestamp_millis());
    }
    if let Some(to) = query.to {
        q = q.bind(to.timestamp_millis());
    }

    q = q.bind(query.limit.unwrap_or(100).min(1000));

    let events = q
        .fetch_all::<LogRow>()
        .await
        .map_err(|source| ApiError::Storage {
            operation: "read logs",
            source,
        })?;

    let result = events
        .iter()
        .map(LogEvent::try_from)
        .collect::<Result<Vec<LogEvent>, LogRowConversionError>>()?;

    Ok(Json(result))
}

async fn post_handler(
    State(state): State<AppState>,
    body: Result<Json<LogEvent>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(body) = body?;
    validate_service(&body.service, None)?;
    let storage_error = |source| ApiError::Storage {
        operation: "store log",
        source,
    };
    let mut insert = state
        .client
        .insert::<LogRow>("logs")
        .await
        .map_err(storage_error)?;
    insert
        .write(&LogRow::from(body))
        .await
        .map_err(storage_error)?;
    insert.end().await.map_err(storage_error)?;

    Ok(StatusCode::CREATED)
}

async fn batch_post_handler(
    State(state): State<AppState>,
    body: Result<Json<Vec<LogEvent>>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(body) = body?;
    if body.is_empty() {
        return Err(ApiError::InvalidRequest {
            message: "batch must contain at least one event",
            field: None,
            event_index: None,
        });
    }
    if body.len() > MAX_BATCH_EVENTS {
        return Err(ApiError::BatchTooLarge {
            limit: MAX_BATCH_EVENTS,
        });
    }
    for (index, event) in body.iter().enumerate() {
        validate_service(&event.service, Some(index))?;
    }

    let storage_error = |source| ApiError::Storage {
        operation: "store log batch",
        source,
    };
    let mut inserter = state.client.inserter::<LogRow>("logs");
    for event in body {
        inserter
            .write(&LogRow::from(event))
            .await
            .map_err(storage_error)?;
    }
    inserter.end().await.map_err(storage_error)?;
    Ok(StatusCode::CREATED)
}

fn validate_service(service: &str, event_index: Option<usize>) -> Result<(), ApiError> {
    if service.trim().is_empty() {
        return Err(ApiError::InvalidRequest {
            message: "service must not be blank",
            field: Some("service"),
            event_index,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
