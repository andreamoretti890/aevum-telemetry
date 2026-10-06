use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

#[tokio::main]
async fn main() {
    let app = create_app();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    println!("Server running on http://127.0.0.1:3000");

    axum::serve(listener, app).await.unwrap();
}

fn create_app() -> Router {
    Router::<()>::new().route("/health", get(health))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

#[tokio::test]
async fn test_ping() {
    use axum_test::TestServer;

    let app = create_app();

    let server = TestServer::new(app);
    let response = server.get("/health").await;

    response.assert_status_ok();
    response.assert_json(&json!({"status": "ok"}));
}
