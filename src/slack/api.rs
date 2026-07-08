//! Slack Web API helpers shared by the transport layer.

use serde::Serialize;
use serde_json::Value;

/// POST to `https://slack.com/api/{method}` and return the JSON body.
pub fn slack_api_post(token: &str, method: &str, body: &impl Serialize) -> anyhow::Result<Value> {
    let payload = serde_json::to_string(body)?;
    let response = ureq::post(&format!("https://slack.com/api/{method}"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json; charset=utf-8")
        .send_string(&payload)?;
    if response.status() >= 400 {
        anyhow::bail!("{method} HTTP {}", response.status());
    }
    let value: Value = serde_json::from_str(&response.into_string()?)?;
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown_error");
        anyhow::bail!("{method} failed: {error}");
    }
    Ok(value)
}

/// Run a blocking Slack API call on the runtime's blocking pool.
pub async fn slack_api_post_async(
    token: String,
    method: String,
    body: Value,
) -> anyhow::Result<Value> {
    tokio::task::spawn_blocking(move || slack_api_post(&token, &method, &body)).await?
}