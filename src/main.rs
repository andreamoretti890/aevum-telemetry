mod models;

use axum::{Json, Router, routing::get};
use chrono::Utc;
use serde_json::{Value, json};

use crate::models::log_event::LogEvent;

#[tokio::main]
async fn main() {
    //     let app = create_app();
    //     let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
    //         .await
    //         .unwrap();
    //
    //     println!("Server running on http://127.0.0.1:3000");
    //
    //     axum::serve(listener, app).await.unwrap();
    let event = LogEvent::new(
        Utc::now(),
        models::log_event::LogLevel::Error,
        "Lievito",
        "Printer connection failed",
    );

    let serialized = serde_json::to_string(&event).unwrap();
    println!("serialized = {}", serialized);

    let deserialized: LogEvent = serde_json::from_str(&serialized).unwrap();
    println!("deserialized = {:?}", deserialized);
}

fn create_app() -> Router {
    Router::<()>::new().route("/health", get(health))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

#[tokio::test]
async fn test_health() {
    use axum_test::TestServer;

    let app = create_app();

    let server = TestServer::new(app);
    let response = server.get("/health").await;

    response.assert_status_ok();
    response.assert_json(&json!({"status": "ok"}));
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
