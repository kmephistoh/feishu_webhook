use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;

// 1. 定义 TrapFall 发送过来的 JSON 结构
#[derive(Deserialize)]
struct TrapFallWebhook {
    rule_id: String,
    issue_id: String,
    project_id: String,
    title: String,
    level: String,
    count: u32,
    #[allow(dead_code)] // 忽略此字段的未使用警告，仅用于 JSON 反序列化占位
    action: String,
    triggered_at: String,
}

// 2. 定义飞书机器人要求的 JSON 结构
#[derive(Serialize)]
struct FeishuMessage {
    msg_type: String,
    content: FeishuContent,
}

#[derive(Serialize)]
struct FeishuContent {
    text: String,
}

#[tokio::main]
async fn main() {
    // 获取端口，如果没有设置环境变量 PORT，默认使用 3000
    let port = env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    println!("Webhook Adapter running on port {}", port);

    let app = Router::new().route("/", post(handle_trapfall_alert));

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port))
        .await
        .expect("Failed to bind address");
    axum::serve(listener, app)
        .await
        .expect("Failed to start server");
}

async fn handle_trapfall_alert(
    Json(trapfall_payload): Json<TrapFallWebhook>,
) -> impl IntoResponse {
    // ⚠️ 请在这里填入你的真实飞书 Webhook 地址
    let feishu_url = "https://open.feishu.cn/open-apis/bot/v2/hook/你的飞书机器人Token";

    // 构造飞书能识别的纯文本消息（确保包含"告警"二字以匹配飞书关键词过滤）
    let msg_text = format!(
        r#"【告警】TrapFall 触发告警 🔔
规则ID: {}
IssueID: {}
项目ID: {}
告警等级: {}
错误标题: {}
触发次数: {}
触发时间: {}"#,
        trapfall_payload.rule_id,
        trapfall_payload.issue_id,
        trapfall_payload.project_id,
        trapfall_payload.level.to_uppercase(),
        trapfall_payload.title,
        trapfall_payload.count,
        trapfall_payload.triggered_at
    );

    let feishu_payload = FeishuMessage {
        msg_type: "text".to_string(),
        content: FeishuContent { text: msg_text },
    };

    // 向飞书发送 HTTP POST 请求
    let client = Client::new();
    match client
        .post(feishu_url)
        .json(&feishu_payload)
        .send()
        .await
    {
        Ok(res) => {
            println!("飞书转发成功，HTTP 状态码: {}", res.status());
            StatusCode::OK
        }
        Err(e) => {
            eprintln!("转发飞书失败: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}