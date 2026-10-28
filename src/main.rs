use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use reqwest::Client;
use serde::Deserialize;
use std::env;
use std::sync::Arc;

#[derive(Deserialize)]
struct TrapFallWebhook {
    rule_id: String,
    issue_id: String,
    project_id: String,
    title: String,
    level: String,
    count: u32,
    #[allow(dead_code)]
    action: String,
    triggered_at: String,
}

struct AppConfig {
    listen: String,
    feishu_webhook_url: String,
}

#[tokio::main]
async fn main() {
    let listen = env::var("LISTEN").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
    let feishu_webhook_url = env::var("FEISHU_WEBHOOK_URL")
        .expect("FEISHU_WEBHOOK_URL must be set");

    let config = Arc::new(AppConfig {
        listen,
        feishu_webhook_url,
    });

    println!("Listening on {}", config.listen);

    let app = Router::new()
        .route("/", post(handle_trapfall_alert))
        .with_state(Arc::clone(&config));

    let listener = tokio::net::TcpListener::bind(&config.listen)
        .await
        .expect("Failed to bind address");

    axum::serve(listener, app)
        .await
        .expect("Failed to start server");
}

async fn handle_trapfall_alert(
    State(config): State<Arc<AppConfig>>,
    Json(trapfall_payload): Json<TrapFallWebhook>,
) -> impl IntoResponse {
    let msg_text = format!(
        "【告警】TrapFall 触发告警\n规则ID: {}\nIssueID: {}\n项目ID: {}\n等级: {}\n标题: {}\n次数: {}\n时间: {}",
        trapfall_payload.rule_id,
        trapfall_payload.issue_id,
        trapfall_payload.project_id,
        trapfall_payload.level,
        trapfall_payload.title,
        trapfall_payload.count,
        trapfall_payload.triggered_at
    );

    let feishu_payload = serde_json::json!({
        "msg_type": "text",
        "content": {
            "text": msg_text
        }
    });

    let client = Client::new();
    let res = client
        .post(&config.feishu_webhook_url)
        .json(&feishu_payload)
        .send()
        .await;

    match res {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            eprintln!("Feishu response status={status} body={body}");

            if status.is_success() {
                StatusCode::OK
            } else {
                StatusCode::BAD_GATEWAY
            }
        }
        Err(e) => {
            eprintln!("Forward to Feishu failed: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}