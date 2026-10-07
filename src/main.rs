mod models;

use std::{env, sync::Arc};

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::get,
};
use chrono::Utc;
use clickhouse::{Client, insert::Insert, sql::Identifier};
use serde_json::{Value, json};
use tokio::{net::TcpListener, sync::RwLock};

use crate::models::log_event::{LogEvent, LogLevel, LogQuery, LogRowConversionError};
use crate::models::log_row::LogRow;

#[derive(Clone, Default)]
struct AppState {
    events: Arc<RwLock<Vec<LogEvent>>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::default()
        // Match the Rust client's LZ4 decoder; ClickHouse 26.9 defaults to ZSTD.
        .with_setting("network_compression_method", "lz4")
        .with_url(env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://127.0.0.1:8123".into()))
        .with_database(read_env_var("CLICKHOUSE_DB"))
        .with_user(read_env_var("CLICKHOUSE_USER"))
        .with_password(read_env_var("CLICKHOUSE_PASSWORD"));

    let table_name = "logs";
    let mut insert: Insert<LogRow> = client.insert::<LogRow>(table_name).await?;
    insert
        .write(&LogRow::from(LogEvent::new(
            Utc::now(),
            LogLevel::Debug,
            "service-test1",
            "Test2",
        )))
        .await?;
    insert
        .write(&LogRow::from(LogEvent::new(
            Utc::now(),
            LogLevel::Debug,
            "service-test2",
            "Test2",
        )))
        .await?;
    insert.end().await?;

    let logs = client
        .query("SELECT ?fields FROM ? ORDER BY timestamp DESC")
        .bind(Identifier(table_name))
        .fetch_all::<LogRow>()
        .await?;

    let log_events = logs
        .iter()
        .map(LogEvent::try_from)
        .collect::<Result<Vec<LogEvent>, LogRowConversionError>>()?;
    println!("{log_events:#?}");

    let state: AppState = AppState::default();
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
        .with_state(state)
}

async fn health_handler() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn get_handler(
    State(state): State<AppState>,
    Query(query): Query<LogQuery>,
) -> Json<Vec<LogEvent>> {
    let events = state.events.read().await;
    let result: Vec<LogEvent> = events
        .iter()
        .filter(|event| match &query.level {
            Some(level) => event.level == *level,
            None => true,
        })
        .filter(|event| match &query.service {
            Some(service) => event.service == *service,
            None => true,
        })
        .take(query.limit.unwrap_or(100).min(1000))
        .cloned()
        .collect();

    Json(result)
}

async fn post_handler(State(state): State<AppState>, Json(body): Json<LogEvent>) -> StatusCode {
    let mut events = state.events.write().await;
    events.push(body);

    StatusCode::CREATED
}

#[cfg(test)]
mod tests {
    use axum_test::TestServer;
    use chrono::Utc;

    use crate::models::log_event::LogLevel;

    use super::*;

    fn filter_events() -> Vec<LogEvent> {
        [
            (LogLevel::Info, "Other", "Other service started"),
            (LogLevel::Error, "Lievito", "Printer connection failed"),
            (LogLevel::Error, "Other", "Other service failed"),
            (LogLevel::Info, "Lievito", "Lievito started"),
        ]
        .into_iter()
        .map(|(level, service, message)| {
            LogEvent::new(
                "2026-10-06T13:26:13Z".parse().unwrap(),
                level,
                service,
                message,
            )
        })
        .collect()
    }

    fn server_with_events(events: Vec<LogEvent>) -> TestServer {
        let state = AppState {
            events: Arc::new(RwLock::new(events)),
        };
        TestServer::new(create_app(state))
    }

    #[tokio::test]
    async fn test_get_logs_empty() {
        let server = server_with_events(vec![]);

        let response = server.get("/v1/logs").await;

        response.assert_status_ok();
        response.assert_json(&json!([]));
    }

    #[tokio::test]
    async fn test_post_then_get_logs() {
        let server = server_with_events(vec![]);
        let event = filter_events().remove(1);

        let response = server.post("/v1/logs").json(&event).await;
        response.assert_status(StatusCode::CREATED);

        let response = server.get("/v1/logs").await;

        response.assert_status_ok();
        response.assert_json(&vec![event]);
    }

    #[tokio::test]
    async fn test_get_logs_filter_by_level() {
        let events = filter_events();
        let server = server_with_events(events.clone());

        let response = server.get("/v1/logs?level=error").await;

        response.assert_status_ok();
        response.assert_json(&vec![events[1].clone(), events[2].clone()]);
    }

    #[tokio::test]
    async fn test_get_logs_filter_by_service() {
        let events = filter_events();
        let server = server_with_events(events.clone());

        let response = server.get("/v1/logs?service=Lievito").await;

        response.assert_status_ok();
        response.assert_json(&vec![events[1].clone(), events[3].clone()]);
    }

    #[tokio::test]
    async fn test_get_logs_filter_by_level_and_service() {
        let events = filter_events();
        let server = server_with_events(events.clone());

        let response = server.get("/v1/logs?level=error&service=Lievito").await;

        response.assert_status_ok();
        response.assert_json(&vec![events[1].clone()]);
    }

    #[tokio::test]
    async fn test_get_logs_limit() {
        let events = filter_events();
        let server = server_with_events(events.clone());

        let response = server.get("/v1/logs?limit=2").await;

        response.assert_status_ok();
        response.assert_json(&events[..2].to_vec());
    }

    #[tokio::test]
    async fn test_get_logs_limit_capped_at_1000() {
        let events: Vec<LogEvent> = (0..1001)
            .map(|index| {
                LogEvent::new(
                    "2026-10-06T13:26:13Z".parse().unwrap(),
                    LogLevel::Error,
                    "Lievito",
                    &format!("Event {index}"),
                )
            })
            .collect();
        let server = server_with_events(events.clone());

        let response = server.get("/v1/logs?limit=50000").await;

        response.assert_status_ok();
        response.assert_json(&events[..1000].to_vec());
    }

    #[tokio::test]
    async fn test_get_logs_invalid_level() {
        let server = server_with_events(filter_events());

        let response = server.get("/v1/logs?level=invalid").await;

        assert!(response.status_code().is_client_error());
    }

    #[tokio::test]
    async fn test_health() {
        use axum_test::TestServer;

        let state = AppState::default();
        let app = create_app(state);

        let server = TestServer::new(app);
        let response = server.get("/health").await;

        response.assert_status_ok();
        response.assert_json(&json!({"status": "ok"}));
    }

    #[tokio::test]
    async fn test_post_logs() {
        use axum_test::TestServer;

        let state: AppState = AppState::default();
        let app = create_app(state.clone());

        let server = TestServer::new(app);
        let payload = LogEvent::new(
            Utc::now(),
            models::log_event::LogLevel::Error,
            "Lievito",
            "Printer connection failed",
        );

        let response = server.post("/v1/logs").json(&payload).await;
        response.assert_status(StatusCode::CREATED);

        let events = state.events.read().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, "Printer connection failed");
    }

    #[tokio::test]
    async fn test_post_invalid_log() {
        use axum_test::TestServer;

        let state = AppState::default();
        let app = create_app(state.clone());
        let server = TestServer::new(app);

        let response = server
            .post("/v1/logs")
            .json(&json!({
                "timestamp": "2026-10-06T13:26:13Z",
                "level": "critical",
                "service": "Lievito",
                "message": "Printer connection failed"
            }))
            .await;

        assert!(response.status_code().is_client_error());

        let events = state.events.read().await;
        assert!(events.is_empty());
    }

    #[test]
    fn test_serialize() {
        use crate::models::log_event::LogLevel;

        let event = LogEvent::new(
            "2026-10-06T13:26:13Z".parse().unwrap(),
            LogLevel::Error,
            "Lievito",
            "Printer connection failed",
        );

        let json = serde_json::to_value(&event).unwrap();

        assert_eq!(json["timestamp"], "2026-10-06T13:26:13Z");
        assert_eq!(json["level"], "error");
        assert_eq!(json["service"], "Lievito");
        assert_eq!(json["message"], "Printer connection failed");
    }

    #[test]
    fn test_deserialize() {
        let json = r#"
        {
            "timestamp": "2026-10-06T13:26:13Z",
            "level": "error",
            "service": "lievito",
            "message": "Printer failed"
        }
        "#;

        let event = serde_json::from_str::<LogEvent>(json).unwrap();

        assert_eq!(event.level, LogLevel::Error);
        assert_eq!(event.service, "lievito");
        assert_eq!(event.message, "Printer failed");
    }

    #[test]
    fn test_invalid_level() {
        let json = r#"
            {
                "timestamp": "2026-10-06T13:26:13Z",
                "level": "critical",
                "service": "lievito",
                "message": "Printer failed"
            }
            "#;

        let result = serde_json::from_str::<LogEvent>(json);

        assert!(result.is_err());
    }

    #[test]
    fn test_missing_service() {
        let json = r#"
            {
                "timestamp": "2026-10-06T13:26:13Z",
                "level": "error",
                "message": "Printer failed"
            }
            "#;

        let result = serde_json::from_str::<LogEvent>(json);

        assert!(result.is_err());
    }

    #[test]
    fn test_malformed_timestamp() {
        let json = r#"
            {
                "timestamp": "2026-10-:13Z",
                "level": "error",
                "service": "lievito",
                "message": "Printer failed"
            }
            "#;

        let result = serde_json::from_str::<LogEvent>(json);

        assert!(result.is_err());
    }
}
