use std::sync::Arc;

use aes::Aes256;
use axum::{
    body::{to_bytes, Body},
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose, Engine as _};
use cbc::{
    cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit},
    Decryptor,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest as Sha2Digest, Sha256};
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
pub(super) const VERIFICATION_TOKEN_SECRET_KEY: &str = "verification_token";
const ENCRYPT_KEY_SECRET_KEY: &str = "encrypt_key";

type Aes256CbcDec = Decryptor<Aes256>;

#[derive(Debug, Deserialize)]
struct CallbackBody {
    token: Option<String>,
    challenge: Option<String>,
    #[serde(rename = "type")]
    body_type: Option<String>,
    encrypt: Option<String>,
    header: Option<EventHeader>,
    event: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct EventHeader {
    event_id: Option<String>,
    event_type: Option<String>,
    token: Option<String>,
}

pub(super) async fn callback(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<DbId>,
    headers: HeaderMap,
    body: Body,
) -> AppResult<Response> {
    let runtime = runtime_for_endpoint(&state, endpoint_id, "feishu").await?;
    let bytes = to_bytes(body, APP_BODY_LIMIT_BYTES)
        .await
        .map_err(|err| AppError::BadRequest(format!("failed to read request body: {err}")))?;
    let payload = parse_callback(&state, &runtime, &headers, &bytes)?;
    verify_token(&state, &runtime, &payload)?;

    if payload.challenge.is_some() || payload.body_type.as_deref() == Some("url_verification") {
        let challenge = payload
            .challenge
            .ok_or_else(|| AppError::BadRequest("missing feishu challenge".to_string()))?;
        return Ok(Json(json!({ "challenge": challenge })).into_response());
    }

    let event_type = payload
        .header
        .as_ref()
        .and_then(|header| header.event_type.as_deref())
        .unwrap_or("");
    if event_type != "im.message.receive_v1" {
        return Ok("success".into_response());
    }
    let Some(message) = incoming_message(&payload) else {
        return Ok("success".into_response());
    };
    let target_user = message.external_user_id.clone();
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        match run_app_message(Arc::clone(&state_clone), runtime.clone(), message).await {
            Ok(outcome) if !outcome.duplicate => {
                if let Err(err) =
                    send_text(&state_clone, &runtime, &target_user, &outcome.message).await
                {
                    tracing::warn!(endpoint_id = runtime.endpoint_id, error = %err, "failed to send feishu app message");
                }
            }
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(endpoint_id = runtime.endpoint_id, error = %err, "failed to handle feishu app message")
            }
        }
    });
    Ok("success".into_response())
}

pub(super) fn validate_endpoint_config(
    config: &Value,
    has_app_secret: bool,
    has_verification_token: bool,
) -> AppResult<()> {
    let app_id = config
        .get("app_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if app_id.is_empty() {
        return Err(AppError::BadRequest(
            "Feishu App ID is required".to_string(),
        ));
    }
    if !has_app_secret {
        return Err(AppError::BadRequest(
            "Feishu App Secret is required".to_string(),
        ));
    }
    if !has_verification_token {
        return Err(AppError::BadRequest(
            "Feishu Verification Token is required".to_string(),
        ));
    }
    Ok(())
}

fn parse_callback(
    state: &AppState,
    runtime: &AppRuntime,
    headers: &HeaderMap,
    bytes: &[u8],
) -> AppResult<CallbackBody> {
    let raw: CallbackBody = serde_json::from_slice(bytes)
        .map_err(|_| AppError::BadRequest("invalid feishu callback body".to_string()))?;
    let encrypt_key = secret_plaintext(state, runtime, ENCRYPT_KEY_SECRET_KEY)?;
    let Some(encrypted) = raw.encrypt.as_deref() else {
        return Ok(raw);
    };
    if !encrypt_key.is_empty() {
        verify_signature(&encrypt_key, headers, bytes)?;
    }
    decrypt_payload(&encrypt_key, encrypted)
}

fn verify_token(state: &AppState, runtime: &AppRuntime, payload: &CallbackBody) -> AppResult<()> {
    let token = secret_plaintext(state, runtime, VERIFICATION_TOKEN_SECRET_KEY)?;
    if token.is_empty() {
        return Ok(());
    }
    let received = payload
        .token
        .as_deref()
        .or_else(|| {
            payload
                .header
                .as_ref()
                .and_then(|header| header.token.as_deref())
        })
        .ok_or(AppError::Unauthorized)?;
    constant_time_eq(received.as_bytes(), token.as_bytes())
        .then_some(())
        .ok_or(AppError::Unauthorized)
}

fn verify_signature(encrypt_key: &str, headers: &HeaderMap, bytes: &[u8]) -> AppResult<()> {
    let timestamp = header_value(headers, "x-lark-request-timestamp")
        .or_else(|| header_value(headers, "x-tt-request-timestamp"))
        .ok_or(AppError::Unauthorized)?;
    let nonce = header_value(headers, "x-lark-request-nonce")
        .or_else(|| header_value(headers, "x-tt-request-nonce"))
        .ok_or(AppError::Unauthorized)?;
    let signature = header_value(headers, "x-lark-signature")
        .or_else(|| header_value(headers, "x-tt-signature"))
        .ok_or(AppError::Unauthorized)?;
    let expected = callback_signature(timestamp, nonce, encrypt_key, bytes);
    constant_time_eq(signature.as_bytes(), expected.as_bytes())
        .then_some(())
        .ok_or(AppError::Unauthorized)
}

fn header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn callback_signature(timestamp: &str, nonce: &str, encrypt_key: &str, body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(timestamp.as_bytes());
    hasher.update(nonce.as_bytes());
    hasher.update(encrypt_key.as_bytes());
    hasher.update(body);
    hex::encode(hasher.finalize())
}

fn decrypt_payload(encrypt_key: &str, encrypted: &str) -> AppResult<CallbackBody> {
    if encrypt_key.is_empty() {
        return Err(AppError::BadRequest(
            "Feishu Encrypt Key is required".to_string(),
        ));
    }
    let key = Sha256::digest(encrypt_key.as_bytes());
    let mut ciphertext = general_purpose::STANDARD
        .decode(encrypted)
        .map_err(|_| AppError::BadRequest("invalid feishu encrypted payload".to_string()))?;
    let decrypted = Aes256CbcDec::new_from_slices(&key, &key[..16])
        .map_err(|_| AppError::BadRequest("invalid feishu encrypt key".to_string()))?
        .decrypt_padded_mut::<Pkcs7>(&mut ciphertext)
        .map_err(|_| AppError::BadRequest("failed to decrypt feishu payload".to_string()))?;
    serde_json::from_slice(decrypted)
        .map_err(|_| AppError::BadRequest("invalid decrypted feishu json".to_string()))
}

fn incoming_message(payload: &CallbackBody) -> Option<IncomingAppMessage> {
    let event = payload.event.as_ref()?;
    let message = event.get("message")?;
    let message_type = message.get("message_type")?.as_str()?;
    if message_type != "text" {
        return None;
    }
    let content = parse_text_content(message.get("content")?.as_str()?)?;
    let sender_id = event.get("sender")?.get("sender_id")?;
    let open_id = sender_id.get("open_id")?.as_str()?.to_string();
    let chat_id = message
        .get("chat_id")
        .and_then(Value::as_str)
        .unwrap_or(&open_id)
        .to_string();
    let message_id = message
        .get("message_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            payload
                .header
                .as_ref()
                .and_then(|header| header.event_id.clone())
        });
    Some(IncomingAppMessage {
        external_user_id: open_id,
        external_conversation_id: chat_id,
        external_message_id: message_id,
        content,
        metadata: json!({ "source": "feishu" }),
        trace_id: Uuid::new_v4().to_string(),
    })
}

fn parse_text_content(value: &str) -> Option<String> {
    serde_json::from_str::<Value>(value)
        .ok()
        .and_then(|content| {
            content
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| Some(value.to_string()))
}

async fn send_text(
    state: &AppState,
    runtime: &AppRuntime,
    target_open_id: &str,
    content: &str,
) -> AppResult<()> {
    let app_id = runtime
        .endpoint_config
        .get("app_id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("Feishu App ID is required".to_string()))?;
    let app_secret = secret_plaintext(state, runtime, APP_SECRET_KEY)?;
    let token_value: Value = state
        .http
        .post("https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal")
        .json(&json!({
            "app_id": app_id,
            "app_secret": app_secret,
        }))
        .send()
        .await?
        .json()
        .await?;
    if token_value.get("code").and_then(Value::as_i64).unwrap_or(0) != 0 {
        return Err(AppError::BadRequest(format!(
            "feishu token failed: {token_value}"
        )));
    }
    let access_token = token_value
        .get("tenant_access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("failed to fetch feishu tenant token".to_string()))?;
    let res: Value = state
        .http
        .post("https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type=open_id")
        .bearer_auth(access_token)
        .json(&json!({
            "receive_id": target_open_id,
            "msg_type": "text",
            "content": json!({ "text": content }).to_string(),
        }))
        .send()
        .await?
        .json()
        .await?;
    if res.get("code").and_then(Value::as_i64).unwrap_or(0) != 0 {
        return Err(AppError::BadRequest(format!("feishu send failed: {res}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::{
        cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit},
        Encryptor,
    };

    #[test]
    fn signature_uses_timestamp_nonce_encrypt_key_and_body() {
        let body = br#"{"encrypt":"payload"}"#;
        let signature = callback_signature("1700000000", "nonce", "encrypt-key", body);

        assert_eq!(
            signature,
            "5ae256847fc52c6239f34ad914c3e5cd08431e572b45f1dbf9272efafe716d25"
        );
    }

    #[test]
    fn decrypt_payload_extracts_challenge() {
        let encrypted = encrypt_payload(
            "encrypt-key",
            r#"{"token":"verify-token","challenge":"challenge-code","type":"url_verification"}"#,
        );

        let payload = decrypt_payload("encrypt-key", &encrypted).expect("decrypt payload");

        assert_eq!(payload.token.as_deref(), Some("verify-token"));
        assert_eq!(payload.challenge.as_deref(), Some("challenge-code"));
        assert_eq!(payload.body_type.as_deref(), Some("url_verification"));
    }

    #[test]
    fn incoming_message_extracts_text() {
        let payload: CallbackBody = serde_json::from_value(json!({
            "schema": "2.0",
            "header": {
                "event_id": "event-1",
                "event_type": "im.message.receive_v1",
                "token": "verify-token"
            },
            "event": {
                "sender": {
                    "sender_id": { "open_id": "ou_user" }
                },
                "message": {
                    "message_id": "om_msg",
                    "chat_id": "oc_chat",
                    "message_type": "text",
                    "content": "{\"text\":\"hello feishu\"}"
                }
            }
        }))
        .expect("valid payload");

        let message = incoming_message(&payload).expect("incoming message");

        assert_eq!(message.external_user_id, "ou_user");
        assert_eq!(message.external_conversation_id, "oc_chat");
        assert_eq!(message.external_message_id.as_deref(), Some("om_msg"));
        assert_eq!(message.content, "hello feishu");
    }

    fn encrypt_payload(encrypt_key: &str, json: &str) -> String {
        let key = Sha256::digest(encrypt_key.as_bytes());
        let mut plaintext = json.as_bytes().to_vec();
        let len = plaintext.len();
        plaintext.resize(len + 16, 0);
        let encrypted = Encryptor::<Aes256>::new_from_slices(&key, &key[..16])
            .expect("valid cipher")
            .encrypt_padded_mut::<Pkcs7>(&mut plaintext, len)
            .expect("encrypt payload");
        general_purpose::STANDARD.encode(encrypted)
    }
}
