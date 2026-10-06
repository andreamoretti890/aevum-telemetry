mod models;

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde_json::{Value, json};
use tokio::{net::TcpListener, sync::Mutex};

use crate::models::log_event::LogEvent;

#[derive(Clone, Default)]
struct AppState {
    events: Arc<Mutex<Vec<LogEvent>>>,
}

#[tokio::main]
async fn main() {
    let state: AppState = AppState::default();
    let app: Router = create_app(state);
    let listener: TcpListener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    println!("Server running on http://127.0.0.1:3000");

    axum::serve(listener, app).await.unwrap();
}

fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/v1/logs", post(post_handler))
        .with_state(state)
}

async fn health_handler() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn post_handler(State(state): State<AppState>, Json(body): Json<LogEvent>) -> StatusCode {
    let mut events = state.events.lock().await;
    events.push(body);

    StatusCode::CREATED
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

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

        let events = state.events.lock().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, "Printer connection failed");
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
                "timesta: "2026-10-06T13:26:13Z",
                "level": "critical",
                "service": "lievito",
                "message": "Printer failed"
            }
            "#;

        let result = serde_json::from_str::<LogEvent>(json);

        assert!(result.is_err());
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
                "message": "Printer failed"
            }
            "#;

        let result = serde_json::from_str::<LogEvent>(json);

        assert!(result.is_err());
    }
}
