mod models;

use std::env;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post},
};
use clickhouse::{Client, sql::Identifier};
use serde_json::{Value, json};
use tokio::net::TcpListener;

use crate::models::log_event::{LogEvent, LogQuery, LogRowConversionError};
use crate::models::log_row::LogRow;

#[derive(Clone, Default)]
struct AppState {
    client: Client,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

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

    println!("Server running on http://127.0.0.1:3000");

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
        .with_state(state)
}

async fn health_handler() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn get_handler(
    State(state): State<AppState>,
    Query(query): Query<LogQuery>,
) -> Result<Json<Vec<LogEvent>>, StatusCode> {
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
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result = events
        .iter()
        .map(LogEvent::try_from)
        .collect::<Result<Vec<LogEvent>, LogRowConversionError>>()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(result))
}

async fn post_handler(State(state): State<AppState>, Json(body): Json<LogEvent>) -> StatusCode {
    let Ok(mut insert) = state.client.insert::<LogRow>("logs").await else {
        return StatusCode::INTERNAL_SERVER_ERROR;
    };
    if insert.write(&LogRow::from(body)).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    if insert.end().await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    StatusCode::CREATED
}

async fn batch_post_handler(
    State(state): State<AppState>,
    Json(body): Json<Vec<LogEvent>>,
) -> StatusCode {
    let mut inserter = state.client.inserter::<LogRow>("logs");
    for event in body {
        if inserter.write(&LogRow::from(event)).await.is_err() {
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    }
    match inserter.end().await {
        Ok(_) => StatusCode::CREATED,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[cfg(test)]
mod tests;
