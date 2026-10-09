use super::*;
use crate::models::log_event::LogLevel;
use axum_test::{TestResponse, TestServer};
use clickhouse::test::{Mock, handlers};
use std::env;

fn event() -> LogEvent {
    LogEvent::new(
        "2026-10-07T08:00:00.123Z".parse().unwrap(),
        LogLevel::Debug,
        "service-test2",
        "Test",
        json!({"printer": "receipt", "retries": 2, "context": {"online": false}})
            .as_object()
            .unwrap()
            .clone(),
    )
}

fn mock_server(mock: &Mock) -> TestServer {
    TestServer::new(create_app(AppState {
        client: Client::default().with_url(mock.url()),
        table: "logs".into(),
    }))
}

fn assert_api_error(response: &TestResponse, status: StatusCode, code: &str) -> Value {
    response.assert_status(status);
    assert_eq!(response.content_type(), "application/json");
    let body = response.json::<Value>();
    assert_eq!(body["error"]["code"], code);
    assert!(!body["error"]["message"].as_str().unwrap().is_empty());
    uuid::Uuid::parse_str(body["error"]["request_id"].as_str().unwrap()).unwrap();
    body
}

#[tokio::test]
async fn test_get_logs_reads_clickhouse() {
    let mock = Mock::new();
    mock.add(handlers::provide(vec![LogRow::from(event())]));
    let server = mock_server(&mock);
    let response = server.get("/v1/logs").await;
    response.assert_status_ok();
    response.assert_json(&vec![event()]);
}

#[tokio::test]
async fn test_post_logs_writes_clickhouse() {
    let mock = Mock::new();
    let insert = mock.add(handlers::record::<LogRow>());
    let server = mock_server(&mock);
    server
        .post("/v1/logs")
        .json(&event())
        .await
        .assert_status(StatusCode::CREATED);
    assert_eq!(
        insert.collect::<Vec<LogRow>>().await,
        vec![LogRow::from(event())]
    );
}

#[tokio::test]
async fn test_clickhouse_errors_return_500() {
    let mock = Mock::new();
    let server = mock_server(&mock);
    mock.add(handlers::failure(StatusCode::SERVICE_UNAVAILABLE));
    let response = server.get("/v1/logs").await;
    let read_error = assert_api_error(
        &response,
        StatusCode::INTERNAL_SERVER_ERROR,
        "storage_error",
    );
    assert_eq!(read_error["error"]["message"], "failed to read logs");
    for (path, payload, message) in [
        ("/v1/logs", json!(event()), "failed to store log"),
        (
            "/v1/logs/batch",
            json!([event(), event()]),
            "failed to store log batch",
        ),
    ] {
        mock.add(handlers::failure(StatusCode::SERVICE_UNAVAILABLE));
        let response = server.post(path).json(&payload).await;
        let error = assert_api_error(
            &response,
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_error",
        );
        assert_eq!(error["error"]["message"], message);
        assert_ne!(
            error["error"]["request_id"],
            read_error["error"]["request_id"]
        );
    }
}

#[tokio::test]
async fn test_clickhouse_unreachable_returns_500() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let server = TestServer::new(create_app(AppState {
        client: Client::default().with_url(format!("http://{address}")),
        table: "logs".into(),
    }));
    let response = server.get("/v1/logs").await;
    assert_api_error(
        &response,
        StatusCode::INTERNAL_SERVER_ERROR,
        "storage_error",
    );
    for (path, payload) in [
        ("/v1/logs", json!(event())),
        ("/v1/logs/batch", json!([event()])),
    ] {
        let response = server.post(path).json(&payload).await;
        assert_api_error(
            &response,
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_error",
        );
    }
}

#[tokio::test]
async fn test_invalid_requests_do_not_access_clickhouse() {
    let mock = Mock::new();
    let server = mock_server(&mock);
    for query in [
        "level=invalid",
        "limit=-1",
        "service=%20%20",
        "from=2026-10-07T09:00:00Z&to=2026-10-07T08:00:00Z",
    ] {
        let response = server.get(&format!("/v1/logs?{query}")).await;
        assert_api_error(&response, StatusCode::BAD_REQUEST, "invalid_request");
    }
    let mut payload = serde_json::to_value(event()).unwrap();
    payload["level"] = json!("critical");
    let response = server.post("/v1/logs").json(&payload).await;
    assert_api_error(
        &response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request",
    );

    let mut blank_service = event();
    blank_service.service = " \t".into();
    let response = server.post("/v1/logs").json(&blank_service).await;
    let error = assert_api_error(&response, StatusCode::BAD_REQUEST, "invalid_request");
    assert_eq!(error["error"]["field"], "service");

    let response = server
        .post("/v1/logs")
        .text("{")
        .content_type("application/json")
        .await;
    assert_api_error(&response, StatusCode::BAD_REQUEST, "invalid_request");
    let response = server.post("/v1/logs").text("{}").await;
    assert_api_error(
        &response,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "invalid_request",
    );
}

#[tokio::test]
async fn test_batch_validates_all_events_before_writing() {
    // No mock handlers: any attempted database access fails this test.
    let mock = Mock::new();
    let server = mock_server(&mock);
    let response = server
        .post("/v1/logs/batch")
        .json(&Vec::<LogEvent>::new())
        .await;
    assert_api_error(&response, StatusCode::BAD_REQUEST, "invalid_request");

    let mut invalid = event();
    invalid.service = " \n".into();
    let response = server
        .post("/v1/logs/batch")
        .json(&vec![event(), invalid.clone(), invalid])
        .await;
    let error = assert_api_error(&response, StatusCode::BAD_REQUEST, "invalid_request");
    assert_eq!(error["error"]["field"], "service");
    assert_eq!(error["error"]["event_index"], 1);

    let response = server
        .post("/v1/logs/batch")
        .json(&vec![event(); 1001])
        .await;
    let error = assert_api_error(&response, StatusCode::PAYLOAD_TOO_LARGE, "batch_too_large");
    assert_eq!(error["error"]["limit"], 1000);
}

#[tokio::test]
async fn test_batch_accepts_limit_and_preserves_events() {
    let mock = Mock::new();
    let insert = mock.add(handlers::record::<LogRow>());
    let server = mock_server(&mock);
    let mut events = vec![event(); 1000];
    events[0].message.clear();
    events[999].message = "last event".into();
    server
        .post("/v1/logs/batch")
        .json(&events)
        .await
        .assert_status(StatusCode::CREATED);
    assert_eq!(
        insert.collect::<Vec<LogRow>>().await,
        events.into_iter().map(LogRow::from).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_oversized_bodies_return_structured_errors() {
    let mock = Mock::new();
    let server = mock_server(&mock);
    let mut oversized = event();
    oversized.message = "x".repeat(10 * 1024 * 1024);
    for (path, payload) in [
        ("/v1/logs", json!(&oversized)),
        ("/v1/logs/batch", json!([&oversized])),
    ] {
        let response = server.post(path).json(&payload).await;
        let error = assert_api_error(
            &response,
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
        );
        assert_eq!(error["error"]["limit"], 10 * 1024 * 1024);
    }
}

#[tokio::test]
async fn test_invalid_stored_rows_fail_the_whole_query() {
    let mock = Mock::new();
    let server = mock_server(&mock);
    let mut bad_level = LogRow::from(event());
    bad_level.level = 5;
    let mut bad_timestamp = LogRow::from(event());
    bad_timestamp.timestamp = i64::MAX;
    let mut bad_attributes = LogRow::from(event());
    bad_attributes.attributes = "broken JSON".into();
    for row in [bad_level, bad_timestamp, bad_attributes] {
        mock.add(handlers::provide(vec![LogRow::from(event()), row]));
        let response = server.get("/v1/logs").await;
        let error = assert_api_error(
            &response,
            StatusCode::INTERNAL_SERVER_ERROR,
            "stored_log_invalid",
        );
        assert_eq!(
            error["error"]["message"],
            "a stored log could not be decoded"
        );
    }
}

#[tokio::test]
async fn test_routing_errors_use_the_error_envelope() {
    let server = TestServer::new(create_app(AppState::default()));
    let response = server.get("/missing").await;
    assert_api_error(&response, StatusCode::NOT_FOUND, "not_found");
    let response = server.delete("/v1/logs").await;
    assert_api_error(
        &response,
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
    );
}

#[tokio::test]
async fn test_health() {
    let server = TestServer::new(create_app(AppState::default()));
    let response = server.get("/health").await;
    response.assert_status_ok();
    response.assert_json(&json!({"status": "ok"}));
}

#[tokio::test]
#[ignore = "requires local ClickHouse and exported CLICKHOUSE_DB, CLICKHOUSE_USER, CLICKHOUSE_PASSWORD"]
async fn clickhouse_logs_api() {
    let client = Client::default()
        .with_compression(clickhouse::Compression::Lz4)
        .with_setting("network_compression_method", "lz4")
        .with_url(env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://127.0.0.1:8123".into()))
        .with_user(env::var("CLICKHOUSE_USER").expect("CLICKHOUSE_USER must be set"))
        .with_password(env::var("CLICKHOUSE_PASSWORD").expect("CLICKHOUSE_PASSWORD must be set"));
    let database = format!("_aevum_api_test_{}", std::process::id());
    client
        .query("CREATE DATABASE ?")
        .bind(Identifier(&database))
        .execute()
        .await
        .unwrap();
    let test_client = client.clone().with_database(&database);
    // Run assertions in a task so cleanup also happens after an assertion panic.
    let result = tokio::spawn(async move {
        test_client.query("CREATE TABLE logs (timestamp DateTime64(3, 'UTC'), level Enum8('trace' = 0, 'debug' = 1, 'info' = 2, 'warn' = 3, 'error' = 4), service String, message String, attributes String) ENGINE = MergeTree ORDER BY timestamp")
            .execute().await.unwrap();
        let server = TestServer::new(create_app(AppState { client: test_client.clone(), table: "logs".into() }));
        let response = server.get("/v1/logs").await;
        response.assert_status_ok();
        response.assert_json(&Vec::<LogEvent>::new());
        let events: Vec<_> = [
            (3, LogLevel::Info, "service-test1"),
            (1, LogLevel::Debug, "service-test2"),
            (4, LogLevel::Debug, "service-test1"),
            (2, LogLevel::Info, "service-test2"),
        ].into_iter().map(|(second, level, service)| {
            LogEvent::new(format!("2026-10-07T08:00:0{second}.123Z").parse().unwrap(), level, service, "Test", event().attributes)
        }).collect();
        for event in &events {
            server.post("/v1/logs").json(event).await.assert_status(StatusCode::CREATED);
        }
        let rows = test_client.query("SELECT ?fields FROM logs ORDER BY timestamp DESC").fetch_all::<LogRow>().await.unwrap();
        let expected = vec![events[2].clone(), events[0].clone(), events[3].clone(), events[1].clone()];
        assert_eq!(rows.iter().map(LogEvent::try_from).collect::<Result<Vec<_>, _>>().unwrap(), expected);
        for (query, expected) in [
            ("", expected.clone()),
            ("?service=service-test2", vec![events[3].clone(), events[1].clone()]),
            ("?level=debug", vec![events[2].clone(), events[1].clone()]),
            ("?service=service-test2&level=debug", vec![events[1].clone()]),
            ("?limit=1", vec![events[2].clone()]),
            ("?limit=0", vec![]),
            ("?service=missing", vec![]),
            ("?service=service-test2&level=debug&limit=1", vec![events[1].clone()]),
        ] {
            let response = server.get(&format!("/v1/logs{query}")).await;
            response.assert_status_ok();
            response.assert_json(&expected);
        }
        drop(server);
        let server = TestServer::new(create_app(AppState { client: test_client.clone(), table: "logs".into() }));
        let response = server.get("/v1/logs").await;
        response.assert_status_ok();
        response.assert_json(&expected);
        let mut insert = test_client.insert::<LogRow>("logs").await.unwrap();
        for index in 0..1001 {
            let mut row = LogRow::from(event());
            row.timestamp += index * 1000;
            insert.write(&row).await.unwrap();
        }
        insert.end().await.unwrap();
        for (query, count) in [("", 100), ("?limit=50000", 1000)] {
            let response = server.get(&format!("/v1/logs{query}")).await;
            response.assert_status_ok();
            assert_eq!(response.json::<Vec<LogEvent>>().len(), count);
        }
    }).await;
    client
        .query("DROP DATABASE ? SYNC")
        .bind(Identifier(&database))
        .execute()
        .await
        .unwrap();
    result.unwrap();
}

#[test]
fn test_serialize() {
    use crate::models::log_event::LogLevel;

    let event = LogEvent::new(
        "2026-10-06T13:26:13Z".parse().unwrap(),
        LogLevel::Error,
        "Lievito",
        "Printer connection failed",
        event().attributes,
    );

    let json = serde_json::to_value(&event).unwrap();

    assert_eq!(json["timestamp"], "2026-10-06T13:26:13Z");
    assert_eq!(json["level"], "error");
    assert_eq!(json["service"], "Lievito");
    assert_eq!(json["message"], "Printer connection failed");
    assert_eq!(
        json["attributes"],
        serde_json::to_value(&event.attributes).unwrap()
    );
}

#[test]
fn test_deserialize() {
    let json = r#"
        {
            "timestamp": "2026-10-06T13:26:13Z",
            "level": "error",
            "service": "lievito",
            "message": "Printer failed",
            "attributes": {"printer": "receipt", "retries": 2, "context": {"online": false}}
        }
        "#;

    let event = serde_json::from_str::<LogEvent>(json).unwrap();

    assert_eq!(event.level, LogLevel::Error);
    assert_eq!(event.service, "lievito");
    assert_eq!(event.message, "Printer failed");
    assert_eq!(event.attributes, self::event().attributes);
}

#[test]
fn test_invalid_level() {
    let json = r#"
            {
                "timestamp": "2026-10-06T13:26:13Z",
                "level": "critical",
                "service": "lievito",
                "message": "Printer failed",
                "attributes": {}
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
                "message": "Printer failed",
                "attributes": {}
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
                "message": "Printer failed",
                "attributes": {}
            }
            "#;

    let result = serde_json::from_str::<LogEvent>(json);

    assert!(result.is_err());
}
