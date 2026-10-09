mod api_error;
mod models;

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Query, Request, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    routing::{get, post},
};
use clickhouse::{Client, sql::Identifier};
use serde_json::{Value, json};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{
    ServiceBuilderExt,
    request_id::{MakeRequestId, RequestId},
    trace::{DefaultMakeSpan, DefaultOnFailure, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

use crate::models::log_event::{LogEvent, LogQuery, LogRowConversionError};
use crate::models::log_row::LogRow;
use crate::{
    api_error::{ApiError, MAX_BATCH_EVENTS, MAX_BODY_BYTES},
    models::config::Config,
};

#[derive(Clone, Default, Debug)]
struct AppState {
    client: Client,
    table: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let config = Config::from_env()?;
    let address = SocketAddr::new(config.host, config.port);

    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let client = Client::default()
        // Match the Rust client's LZ4 decoder; ClickHouse 26.9 defaults to ZSTD.
        .with_setting("network_compression_method", "lz4")
        .with_url(config.clickhouse_url)
        .with_database(config.clickhouse_db)
        .with_user(config.clickhouse_user)
        .with_password(config.clickhouse_password);

    let state: AppState = AppState {
        client,
        table: config.clickhouse_table,
    };
    let app: Router = create_app(state);
    let listener: TcpListener = tokio::net::TcpListener::bind(&address.to_string()).await?;

    tracing::info!(address = %address, "server listening");

    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Clone, Default)]
struct RequestUuid;

impl MakeRequestId for RequestUuid {
    fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
        let id = Uuid::new_v4().to_string();
        Some(RequestId::new(id.parse().unwrap()))
    }
}

fn create_app(state: AppState) -> Router {
    let trace_layer = ServiceBuilder::new()
        .set_x_request_id(RequestUuid)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .make_span_with(|request: &Request<_>| {
                    let request_id = request
                        .headers()
                        .get("x-request-id")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("unknown");

                    tracing::info_span!(
                        "request",
                        method = %request.method(),
                        uri = %request.uri(),
                        request_id = %request_id,
                    )
                })
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO))
                .on_failure(DefaultOnFailure::new().level(Level::ERROR)),
        )
        .propagate_x_request_id();

    Router::new()
        .route("/health", get(health_handler))
        .route("/v1/logs", get(get_handler).post(post_handler))
        .route("/v1/logs/batch", post(batch_post_handler))
        .fallback(|| async { ApiError::NotFound })
        .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
        .with_state(state)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(trace_layer)
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

    let mut q = state.client.query(&sql).bind(Identifier(&state.table));
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

    tracing::info!(result_count = result.len(), "logs retrieved");

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
        .insert::<LogRow>(&state.table)
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
    let body_len = body.len();
    if body_len > MAX_BATCH_EVENTS {
        return Err(ApiError::BatchTooLarge {
            limit: MAX_BATCH_EVENTS,
        });
    }
    for (index, event) in body.iter().enumerate() {
        validate_service(&event.service, Some(index))?;
    }

    tracing::info!(event_count = body_len, "ingesting log batch");

    let storage_error = |source| ApiError::Storage {
        operation: "store log batch",
        source,
    };
    let mut inserter = state.client.inserter::<LogRow>(&state.table);
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
