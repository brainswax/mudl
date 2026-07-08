//! HTTP server for Slack Event Subscriptions.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde_json::json;
use tracing::warn;

use crate::persistence::Persistence;

use super::bot::SlackBot;
use super::transport::SlackFormattedDelivery;
use super::config::SlackConfig;
use super::events::{parse_events_payload, SlackEventBody, SlackEventsPayload};
use super::verify::verify_slack_signature;

/// Shared state for the Events API HTTP server.
pub struct EventsServerState<P, T> {
    pub bot: Arc<SlackBot<P, T>>,
    pub config: SlackConfig,
}

/// Build the axum router for Slack event subscriptions.
pub fn events_router<P, T>(state: Arc<EventsServerState<P, T>>) -> Router
where
    P: Persistence + Clone + Send + Sync + 'static,
    T: SlackFormattedDelivery + 'static,
{
    let path = state.config.events_path.clone();
    Router::new()
        .route(&path, post(handle_events::<P, T>))
        .with_state(state)
}

/// Run the Events API HTTP server until the process exits.
pub async fn run_events_server<P, T>(
    state: Arc<EventsServerState<P, T>>,
) -> anyhow::Result<()>
where
    P: Persistence + Clone + Send + Sync + 'static,
    T: SlackFormattedDelivery + 'static,
{
    let router = events_router(state.clone());
    let listener = tokio::net::TcpListener::bind(&state.config.bind_addr).await?;
    tracing::info!(
        addr = %state.config.bind_addr,
        path = %state.config.events_path,
        "slack events server listening"
    );
    axum::serve(listener, router).await?;
    Ok(())
}

async fn handle_events<P, T>(
    State(state): State<Arc<EventsServerState<P, T>>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse
where
    P: Persistence + Clone + Send + Sync + 'static,
    T: SlackFormattedDelivery + 'static,
{
    let timestamp = headers
        .get("X-Slack-Request-Timestamp")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let signature = headers
        .get("X-Slack-Signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    if !verify_slack_signature(
        &state.config.signing_secret,
        timestamp,
        &body,
        signature,
    ) {
        warn!("rejected slack event: invalid signature");
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let payload = match parse_events_payload(&String::from_utf8_lossy(&body)) {
        Ok(payload) => payload,
        Err(err) => {
            warn!(error = %err, "failed to parse slack events payload");
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    match payload {
        SlackEventsPayload::UrlVerification { challenge } => {
            (StatusCode::OK, Json(json!({ "challenge": challenge }))).into_response()
        }
        SlackEventsPayload::EventCallback(callback) => {
            let bot = Arc::clone(&state.bot);
            if let SlackEventBody::Message(_) = &callback.event {
                let event = callback.event.clone();
                tokio::spawn(async move {
                    if let Err(err) = bot.handle_event(event).await {
                        warn!(error = %err, "slack event handler error");
                    }
                });
            }
            StatusCode::OK.into_response()
        }
        SlackEventsPayload::Ignored => StatusCode::OK.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::SessionManager;
    use crate::persistence::SqlitePersistence;
    use crate::transport::MockTransport;
    use axum::body::Body;
    use axum::http::Request;
    use hmac::Mac;
    use tower::ServiceExt;

    async fn test_state(secret: &str) -> Arc<EventsServerState<SqlitePersistence, MockTransport>> {
        let persistence = SqlitePersistence::new("sqlite::memory:").await.unwrap();
        let manager = SessionManager::open(persistence, crate::mudl::AnatomyRegistry::default())
            .await
            .unwrap();
        let config = SlackConfig {
            signing_secret: secret.to_string(),
            events_path: "/slack/events".to_string(),
            ..SlackConfig::default()
        };
        let bot = Arc::new(SlackBot::new(
            manager,
            Arc::new(MockTransport::new()),
            config.clone(),
        ));
        Arc::new(EventsServerState { bot, config })
    }

    fn signed_request(
        secret: &str,
        body: &str,
    ) -> (HeaderMap, Bytes) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "0".to_string());
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).expect("hmac");
        mac.update(format!("v0:{timestamp}:{body}").as_bytes());
        let sig = format!(
            "v0={}",
            mac.finalize()
                .into_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Slack-Request-Timestamp",
            timestamp.parse().expect("timestamp"),
        );
        headers.insert("X-Slack-Signature", sig.parse().expect("signature"));
        (headers, Bytes::from(body.to_string()))
    }

    #[tokio::test]
    async fn url_verification_returns_challenge() {
        let secret = "test-secret";
        let state = test_state(secret).await;
        let router = events_router(state);
        let body = r#"{"type":"url_verification","challenge":"challenge-token"}"#;
        let (headers, bytes) = signed_request(secret, body);
        let request = Request::builder()
            .method("POST")
            .uri("/slack/events")
            .body(Body::from(bytes))
            .unwrap();
        // headers need to be applied — use axum test with extensions
        let mut request = request;
        *request.headers_mut() = headers;

        let response = router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rejects_invalid_signature() {
        let state = test_state("secret").await;
        let router = events_router(state);
        let request = Request::builder()
            .method("POST")
            .uri("/slack/events")
            .header("X-Slack-Request-Timestamp", "1")
            .header("X-Slack-Signature", "v0=bad")
            .body(Body::from(r#"{"type":"url_verification","challenge":"x"}"#))
            .unwrap();
        let response = router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}