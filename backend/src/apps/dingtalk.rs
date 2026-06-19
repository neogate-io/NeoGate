use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::Sha256;
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

use super::{
    constant_time_eq, runtime::run_app_message, runtime_for_endpoint, secret_plaintext, AppRuntime,
    IncomingAppMessage, APP_BODY_LIMIT_BYTES,
};

pub(super) const APP_SECRET_KEY: &str = "app_secret";
const SIGNATURE_TOLERANCE_MS: i64 = 60 * 60 * 1000;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Deserialize)]
pub(super) struct CallbackQuery {
    timestamp: Option<String>,
    sign: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CallbackBody {
    msgtype: Option<String>,
    text: Option<TextBody>,
    conversation_id: Option<String>,
    conversation_title: Option<String>,
    conversation_type: Option<String>,
    sender_id: Option<String>,
    sender_staff_id: Option<String>,
    sender_nick: Option<String>,
    msg_id: Option<String>,
    session_webhook: Option<String>,
    session_webhook_expired_time: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TextBody {
    content: Option<String>,
}

struct DingtalkIncoming {
    message: IncomingAppMessage,
    session_webhook: String,
}

pub(super) async fn callback(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<DbId>,
    Query(query): Query<CallbackQuery>,
    headers: HeaderMap,
    body: Body,
) -> AppResult<Response> {
    let runtime = runtime_for_endpoint(&state, endpoint_id, "dingtalk").await?;
    verify_signature(&state, &runtime, &query, &headers)?;
    let bytes = to_bytes(body, APP_BODY_LIMIT_BYTES)
        .await
        .map_err(|err| AppError::BadRequest(format!("failed to read request body: {err}")))?;
    let payload: CallbackBody = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::BadRequest("invalid dingtalk callback body".to_string()))?;
    let Some(incoming) = incoming_message(&payload) else {
        return Ok("success".into_response());
    };

    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        match run_app_message(Arc::clone(&state_clone), runtime.clone(), incoming.message).await {
            Ok(outcome) if !outcome.duplicate => {
                if let Err(err) =
                    send_session_text(&state_clone, &incoming.session_webhook, &outcome.message)
                        .await
                {
                    tracing::warn!(endpoint_id = runtime.endpoint_id, error = %err, "failed to send dingtalk app message");
                }
            }
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(endpoint_id = runtime.endpoint_id, error = %err, "failed to handle dingtalk app message")
            }
        }
    });
    Ok("success".into_response())
}

pub(super) fn validate_endpoint_config(has_app_secret: bool) -> AppResult<()> {
    if !has_app_secret {
        return Err(AppError::BadRequest(
            "DingTalk App Secret is required".to_string(),
        ));
    }
    Ok(())
}

fn verify_signature(
    state: &AppState,
    runtime: &AppRuntime,
    query: &CallbackQuery,
    headers: &HeaderMap,
) -> AppResult<()> {
    let secret = secret_plaintext(state, runtime, APP_SECRET_KEY)?;
    if secret.is_empty() {
        return Err(AppError::BadRequest(
            "DingTalk App Secret is required".to_string(),
        ));
    }
    let timestamp = query
        .timestamp
        .as_deref()
        .or_else(|| header_value(headers, "timestamp"))
        .ok_or(AppError::Unauthorized)?;
    let sign = query
        .sign
        .as_deref()
        .or_else(|| header_value(headers, "sign"))
        .ok_or(AppError::Unauthorized)?;
    let timestamp_ms = timestamp
        .parse::<i64>()
        .map_err(|_| AppError::Unauthorized)?;
    if !timestamp_is_fresh(timestamp_ms, Utc::now().timestamp_millis()) {
        return Err(AppError::Unauthorized);
    }
    let expected = callback_signature(timestamp, &secret);
    constant_time_eq(sign.as_bytes(), expected.as_bytes())
        .then_some(())
        .ok_or(AppError::Unauthorized)
}

fn header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn callback_signature(timestamp: &str, secret: &str) -> String {
    let string_to_sign = format!("{timestamp}\n{secret}");
    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(string_to_sign.as_bytes());
    general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

fn timestamp_is_fresh(timestamp_ms: i64, now_ms: i64) -> bool {
    now_ms.abs_diff(timestamp_ms) <= SIGNATURE_TOLERANCE_MS as u64
}

fn incoming_message(payload: &CallbackBody) -> Option<DingtalkIncoming> {
    if payload.msgtype.as_deref() != Some("text") {
        return None;
    }
    let content = payload
        .text
        .as_ref()?
        .content
        .as_deref()?
        .trim()
        .to_string();
    if content.is_empty() {
        return None;
    }
    let session_webhook = payload.session_webhook.as_ref()?.trim().to_string();
    if session_webhook.is_empty() || session_webhook_expired(payload.session_webhook_expired_time) {
        return None;
    }
    let external_user_id = payload
        .sender_staff_id
        .as_deref()
        .or(payload.sender_id.as_deref())
        .or(payload.sender_nick.as_deref())
        .unwrap_or("dingtalk")
        .to_string();
    let external_conversation_id = payload
        .conversation_id
        .as_deref()
        .or(payload.conversation_title.as_deref())
        .or(payload.conversation_type.as_deref())
        .unwrap_or(&external_user_id)
        .to_string();
    Some(DingtalkIncoming {
        message: IncomingAppMessage {
            external_user_id,
            external_conversation_id,
            external_message_id: payload.msg_id.clone(),
            content,
            metadata: json!({ "source": "dingtalk" }),
            trace_id: Uuid::new_v4().to_string(),
        },
        session_webhook,
    })
}

fn session_webhook_expired(expires_at_ms: Option<i64>) -> bool {
    expires_at_ms.is_some_and(|value| value <= Utc::now().timestamp_millis())
}

async fn send_session_text(
    state: &AppState,
    session_webhook: &str,
    content: &str,
) -> AppResult<()> {
    let res: Value = state
        .http
        .post(session_webhook)
        .json(&json!({
            "msgtype": "text",
            "text": { "content": content },
        }))
        .send()
        .await?
        .json()
        .await?;
    if res.get("errcode").and_then(Value::as_i64).unwrap_or(0) != 0 {
        return Err(AppError::BadRequest(format!("dingtalk send failed: {res}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_signature_uses_timestamp_newline_and_secret() {
        let signature = callback_signature("1700000000000", "app-secret");

        assert_eq!(signature, "3hTZZL5er1Eydgv4cteNysuUXo7Ufcx1jK7Fxfmeq5k=");
    }

    #[test]
    fn timestamp_freshness_allows_one_hour_window() {
        assert!(timestamp_is_fresh(1_700_000_000_000, 1_700_000_030_000));
        assert!(!timestamp_is_fresh(1_700_000_000_000, 1_700_003_700_001));
    }

    #[test]
    fn incoming_message_extracts_text_and_session_webhook() {
        let payload: CallbackBody = serde_json::from_value(json!({
            "msgtype": "text",
            "text": { "content": " hello dingtalk " },
            "conversationId": "cid-1",
            "senderStaffId": "staff-1",
            "msgId": "msg-1",
            "sessionWebhook": "https://oapi.dingtalk.com/robot/sendBySession?session=abc",
            "sessionWebhookExpiredTime": 4_102_444_800_000i64
        }))
        .expect("valid payload");

        let incoming = incoming_message(&payload).expect("incoming message");

        assert_eq!(incoming.message.external_user_id, "staff-1");
        assert_eq!(incoming.message.external_conversation_id, "cid-1");
        assert_eq!(
            incoming.message.external_message_id.as_deref(),
            Some("msg-1")
        );
        assert_eq!(incoming.message.content, "hello dingtalk");
        assert_eq!(
            incoming.session_webhook,
            "https://oapi.dingtalk.com/robot/sendBySession?session=abc"
        );
    }
}
