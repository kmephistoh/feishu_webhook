use axum::{
    extract::{rejection::JsonRejection, State},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use chrono::{DateTime, FixedOffset};
use reqwest::Client;
use serde::Deserialize;
use std::env;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Debug, Deserialize)]
struct TrapFallWebhook {
    #[allow(dead_code)]
    action: String,
    rule: String,
    issue: Issue,
}

#[derive(Debug, Deserialize)]
struct Issue {
    count: u32,
    culprit: String,
    id: String,
    last_seen: String,
    level: String,
    #[allow(dead_code)]
    project_id: String,
    status: String,
    title: String,
}

struct AppConfig {
    listen: String,
    feishu_webhook_url: String,
}

struct DedupCache {
    seen_ids: Vec<(String, Instant)>,
    ttl: Duration,
}

impl DedupCache {
    fn new(ttl_secs: u64) -> Self {
        Self {
            seen_ids: Vec::new(),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    fn is_duplicate(&mut self, id: &str) -> bool {
        let now = Instant::now();
        self.seen_ids.retain(|(_, ts)| now.duration_since(*ts) < self.ttl);
        if self.seen_ids.iter().any(|(sid, _)| sid == id) {
            return true;
        }
        self.seen_ids.push((id.to_string(), now));
        false
    }
}

fn format_time(iso: &str) -> String {
    match DateTime::parse_from_rfc3339(iso) {
        Ok(dt) => {
            let shanghai = FixedOffset::east_opt(8 * 3600).unwrap();
            dt.with_timezone(&shanghai)
                .format("%m-%d %H:%M:%S")
                .to_string()
        }
        Err(_) => iso.to_string(),
    }
}

fn format_culprit(culprit: &str) -> String {
    let parts: Vec<&str> = culprit.splitn(3, ':').collect();
    match parts.as_slice() {
        [file, line, func] => format!("{}:{} ({})", file, line, func),
        [file, line] => format!("{}:{}", file, line),
        _ => culprit.to_string(),
    }
}

#[tokio::main]
async fn main() {
    let listen = env::var("LISTEN").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
    let feishu_webhook_url =
        env::var("FEISHU_WEBHOOK_URL").expect("FEISHU_WEBHOOK_URL must be set");

    let config = Arc::new(AppConfig {
        listen,
        feishu_webhook_url,
    });

    let dedup = Arc::new(Mutex::new(DedupCache::new(300)));

    println!("Listening on {}", config.listen);

    let app = Router::new()
        .route("/", post(handle_trapfall_alert))
        .with_state((Arc::clone(&config), dedup));

    let listener = tokio::net::TcpListener::bind(&config.listen)
        .await
        .expect("Failed to bind address");

    axum::serve(listener, app)
        .await
        .expect("Failed to start server");
}

async fn handle_trapfall_alert(
    State((config, dedup)): State<(Arc<AppConfig>, Arc<Mutex<DedupCache>>)>,
    payload: Result<Json<TrapFallWebhook>, JsonRejection>,
) -> impl IntoResponse {
    let trapfall_payload = match payload {
        Ok(Json(p)) => p,
        Err(e) => {
            eprintln!("TrapFall JSON parse failed: {}", e);
            return StatusCode::UNPROCESSABLE_ENTITY;
        }
    };

    eprintln!("TrapFall payload: {:?}", trapfall_payload);

    {
        let mut cache = dedup.lock().await;
        if cache.is_duplicate(&trapfall_payload.issue.id) {
            eprintln!("Duplicate alert skipped: {}", trapfall_payload.issue.id);
            return StatusCode::OK;
        }
    }

    let prefix = match trapfall_payload.action.as_str() {
        "triggered" | "webhook" => "【告警】",
        "resolved" => "【恢复】",
        _ => "【事件】",
    };

    let msg_text = format!(
        "{} TrapFall 触发告警\n规则: {}\n等级: {}\n标题: {}\n次数: {}\n定位: {}\n状态: {}\n最后出现: {}",
        prefix,
        trapfall_payload.rule,
        trapfall_payload.issue.level,
        trapfall_payload.issue.title,
        trapfall_payload.issue.count,
        format_culprit(&trapfall_payload.issue.culprit),
        trapfall_payload.issue.status,
        format_time(&trapfall_payload.issue.last_seen),
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