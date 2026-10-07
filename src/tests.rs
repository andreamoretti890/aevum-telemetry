use super::*;
use crate::models::log_event::LogLevel;
use axum_test::TestServer;
use clickhouse::test::{Mock, handlers};

fn event() -> LogEvent {
    LogEvent::new(
        "2026-10-07T08:00:00.123Z".parse().unwrap(),
        LogLevel::Debug,
        "service-test2",
        "Test",
    )
}

fn mock_server(mock: &Mock) -> TestServer {
    TestServer::new(create_app(AppState {
        client: Client::default().with_url(mock.url()),
    }))
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
    server
        .get("/v1/logs")
        .await
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    mock.add(handlers::failure(StatusCode::SERVICE_UNAVAILABLE));
    server
        .post("/v1/logs")
        .json(&event())
        .await
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_clickhouse_unreachable_returns_500() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let server = TestServer::new(create_app(AppState {
        client: Client::default().with_url(format!("http://{address}")),
    }));
    server
        .get("/v1/logs")
        .await
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    server
        .post("/v1/logs")
        .json(&event())
        .await
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_invalid_requests_do_not_access_clickhouse() {
    let mock = Mock::new();
    let server = mock_server(&mock);
    server
        .get("/v1/logs?level=invalid")
        .await
        .assert_status(StatusCode::BAD_REQUEST);
    server
        .get("/v1/logs?limit=-1")
        .await
        .assert_status(StatusCode::BAD_REQUEST);
    let mut payload = serde_json::to_value(event()).unwrap();
    payload["level"] = json!("critical");
    server
        .post("/v1/logs")
        .json(&payload)
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);
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
        .with_user(read_env_var("CLICKHOUSE_USER"))
        .with_password(read_env_var("CLICKHOUSE_PASSWORD"));
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
        test_client.query("CREATE TABLE logs (timestamp DateTime64(3, 'UTC'), level Enum8('trace' = 0, 'debug' = 1, 'info' = 2, 'warn' = 3, 'error' = 4), service String, message String) ENGINE = MergeTree ORDER BY timestamp")
            .execute().await.unwrap();
        let server = TestServer::new(create_app(AppState { client: test_client.clone() }));
        let response = server.get("/v1/logs").await;
        response.assert_status_ok();
        response.assert_json(&Vec::<LogEvent>::new());
        let events: Vec<_> = [
            (3, LogLevel::Info, "service-test1"),
            (1, LogLevel::Debug, "service-test2"),
            (4, LogLevel::Debug, "service-test1"),
            (2, LogLevel::Info, "service-test2"),
        ].into_iter().map(|(second, level, service)| {
            LogEvent::new(format!("2026-10-07T08:00:0{second}.123Z").parse().unwrap(), level, service, "Test")
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
        let server = TestServer::new(create_app(AppState { client: test_client.clone() }));
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
