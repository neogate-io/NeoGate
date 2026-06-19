use std::sync::Arc;

use aes::Aes256;
use axum::{
    body::{to_bytes, Body},
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use base64::{engine::general_purpose, Engine as _};
use cbc::{
    cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit},
    Decryptor,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha1::{Digest as Sha1Digest, Sha1};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

use super::{
    constant_time_eq, extract_xml_value, runtime::run_app_message, runtime_for_endpoint,
    secret_plaintext, AppRuntime, IncomingAppMessage, APP_BODY_LIMIT_BYTES,
    WECOM_ENCODING_AES_KEY_ENGINE,
};

pub(super) const TOKEN_SECRET_KEY: &str = "token";
pub(super) const AES_SECRET_KEY: &str = "aes_key";
pub(super) const CORP_SECRET_KEY: &str = "corp_secret";

type Aes256CbcDec = Decryptor<Aes256>;

#[derive(Debug, Deserialize)]
pub(super) struct CallbackQuery {
    msg_signature: Option<String>,
    timestamp: Option<String>,
    nonce: Option<String>,
    echostr: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct WecomDecrypted {
    message: String,
    receive_id: String,
}

pub(super) async fn verify(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<DbId>,
    Query(query): Query<CallbackQuery>,
) -> AppResult<Response> {
    let runtime = runtime_for_endpoint(&state, endpoint_id, "wecom").await?;
    let msg_signature = query
        .msg_signature
        .as_deref()
        .ok_or(AppError::Unauthorized)?;
    let timestamp = query.timestamp.as_deref().ok_or(AppError::Unauthorized)?;
    let nonce = query.nonce.as_deref().ok_or(AppError::Unauthorized)?;
    let echostr = query.echostr.as_deref().ok_or(AppError::Unauthorized)?;
    verify_signature(&state, &runtime, msg_signature, timestamp, nonce, echostr)?;
    let decrypted = decrypt(&state, &runtime, echostr)?;
    verify_receive_id(&runtime, &decrypted.receive_id)?;
    Ok(decrypted.message.into_response())
}

pub(super) async fn message(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<DbId>,
    Query(query): Query<CallbackQuery>,
    body: Body,
) -> AppResult<Response> {
    let runtime = runtime_for_endpoint(&state, endpoint_id, "wecom").await?;
    let bytes = to_bytes(body, APP_BODY_LIMIT_BYTES)
        .await
        .map_err(|err| AppError::BadRequest(format!("failed to read request body: {err}")))?;
    let encrypted = extract_xml_value(&bytes, "Encrypt")
        .ok_or_else(|| AppError::BadRequest("missing Encrypt".to_string()))?;
    let msg_signature = query
        .msg_signature
        .as_deref()
        .ok_or(AppError::Unauthorized)?;
    let timestamp = query.timestamp.as_deref().ok_or(AppError::Unauthorized)?;
    let nonce = query.nonce.as_deref().ok_or(AppError::Unauthorized)?;
    verify_signature(
        &state,
        &runtime,
        msg_signature,
        timestamp,
        nonce,
        &encrypted,
    )?;
    let decrypted = decrypt(&state, &runtime, &encrypted)?;
    verify_receive_id(&runtime, &decrypted.receive_id)?;
    let content = extract_xml_value(decrypted.message.as_bytes(), "Content").unwrap_or_default();
    let msg_type = extract_xml_value(decrypted.message.as_bytes(), "MsgType").unwrap_or_default();
    let from_user = extract_xml_value(decrypted.message.as_bytes(), "FromUserName")
        .unwrap_or_else(|| "wecom".to_string());
    let msg_id = extract_xml_value(decrypted.message.as_bytes(), "MsgId");
    if msg_type != "text" {
        return Ok("success".into_response());
    }

    let message = IncomingAppMessage {
        external_user_id: from_user.clone(),
        external_conversation_id: from_user.clone(),
        external_message_id: msg_id,
        content,
        metadata: json!({ "source": "wecom" }),
        trace_id: Uuid::new_v4().to_string(),
    };
    let state_clone = Arc::clone(&state);
    let target_user = from_user;
    tokio::spawn(async move {
        match run_app_message(Arc::clone(&state_clone), runtime.clone(), message).await {
            Ok(outcome) if !outcome.duplicate => {
                if let Err(err) =
                    send_text(&state_clone, &runtime, &target_user, &outcome.message).await
                {
                    tracing::warn!(endpoint_id = runtime.endpoint_id, error = %err, "failed to send wecom app message");
                }
            }
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(endpoint_id = runtime.endpoint_id, error = %err, "failed to handle wecom app message")
            }
        }
    });
    Ok("success".into_response())
}

pub(super) fn validate_encoding_aes_key(value: &str) -> AppResult<()> {
    if value.len() != 43 {
        return Err(AppError::BadRequest(
            "EncodingAESKey must be 43 characters".to_string(),
        ));
    }
    let key = WECOM_ENCODING_AES_KEY_ENGINE
        .decode(format!("{value}="))
        .map_err(|_| AppError::BadRequest("invalid EncodingAESKey".to_string()))?;
    if key.len() != 32 {
        return Err(AppError::BadRequest("invalid EncodingAESKey".to_string()));
    }
    Ok(())
}

fn verify_signature(
    state: &AppState,
    runtime: &AppRuntime,
    msg_signature: &str,
    timestamp: &str,
    nonce: &str,
    encrypted: &str,
) -> AppResult<()> {
    let token = secret_plaintext(state, runtime, TOKEN_SECRET_KEY)?;
    let expected = signature(&token, timestamp, nonce, encrypted);
    constant_time_eq(msg_signature.as_bytes(), expected.as_bytes())
        .then_some(())
        .ok_or(AppError::Unauthorized)
}

fn signature(token: &str, timestamp: &str, nonce: &str, encrypted: &str) -> String {
    let mut parts = vec![
        token.to_string(),
        timestamp.to_string(),
        nonce.to_string(),
        encrypted.to_string(),
    ];
    parts.sort();
    let mut hasher = Sha1::new();
    for part in parts {
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())
}

fn decrypt(state: &AppState, runtime: &AppRuntime, encrypted: &str) -> AppResult<WecomDecrypted> {
    let aes_key = secret_plaintext(state, runtime, AES_SECRET_KEY)?;
    decrypt_payload(&aes_key, encrypted)
}

fn decrypt_payload(aes_key: &str, encrypted: &str) -> AppResult<WecomDecrypted> {
    let key = WECOM_ENCODING_AES_KEY_ENGINE
        .decode(format!("{aes_key}="))
        .map_err(|_| AppError::BadRequest("invalid EncodingAESKey".to_string()))?;
    if key.len() != 32 {
        return Err(AppError::BadRequest("invalid EncodingAESKey".to_string()));
    }
    let mut ciphertext = general_purpose::STANDARD
        .decode(encrypted)
        .map_err(|_| AppError::BadRequest("invalid encrypted payload".to_string()))?;
    let iv = &key[..16];
    let decrypted = Aes256CbcDec::new_from_slices(&key, iv)
        .map_err(|_| AppError::BadRequest("invalid aes key".to_string()))?
        .decrypt_padded_mut::<NoPadding>(&mut ciphertext)
        .map_err(|_| AppError::BadRequest("failed to decrypt payload".to_string()))?;
    let plaintext = unpad(decrypted)?;
    if plaintext.len() < 20 {
        return Err(AppError::BadRequest(
            "invalid decrypted payload".to_string(),
        ));
    }
    let msg_len =
        u32::from_be_bytes([plaintext[16], plaintext[17], plaintext[18], plaintext[19]]) as usize;
    let start = 20;
    let end = start + msg_len;
    if plaintext.len() < end {
        return Err(AppError::BadRequest(
            "invalid decrypted payload".to_string(),
        ));
    }
    let message = String::from_utf8(plaintext[start..end].to_vec())
        .map_err(|_| AppError::BadRequest("invalid decrypted utf8".to_string()))?;
    let receive_id = String::from_utf8(plaintext[end..].to_vec())
        .map_err(|_| AppError::BadRequest("invalid decrypted utf8".to_string()))?;
    Ok(WecomDecrypted {
        message,
        receive_id,
    })
}

fn verify_receive_id(runtime: &AppRuntime, receive_id: &str) -> AppResult<()> {
    let corp_id = runtime
        .endpoint_config
        .get("corp_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if corp_id.is_empty() {
        return Err(AppError::BadRequest("corp_id is required".to_string()));
    }
    constant_time_eq(receive_id.as_bytes(), corp_id.as_bytes())
        .then_some(())
        .ok_or(AppError::Unauthorized)
}

fn unpad(input: &[u8]) -> AppResult<&[u8]> {
    let Some(&pad) = input.last() else {
        return Err(AppError::BadRequest("empty decrypted payload".to_string()));
    };
    let pad = if pad == 0 { 32 } else { pad as usize };
    if pad > 32 || pad > input.len() {
        return Err(AppError::BadRequest(
            "invalid decrypted padding".to_string(),
        ));
    }
    if !input[input.len() - pad..]
        .iter()
        .all(|value| *value as usize == pad)
    {
        return Err(AppError::BadRequest(
            "invalid decrypted padding".to_string(),
        ));
    }
    Ok(&input[..input.len() - pad])
}

async fn send_text(
    state: &AppState,
    runtime: &AppRuntime,
    target_user: &str,
    content: &str,
) -> AppResult<()> {
    let corp_id = runtime
        .endpoint_config
        .get("corp_id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("corp_id is required".to_string()))?;
    let agent_id = runtime
        .endpoint_config
        .get("agent_id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("agent_id is required".to_string()))?;
    let secret = secret_plaintext(state, runtime, CORP_SECRET_KEY)?;
    let token_url = format!(
        "https://qyapi.weixin.qq.com/cgi-bin/gettoken?corpid={corp_id}&corpsecret={secret}"
    );
    let token_value: Value = state.http.get(token_url).send().await?.json().await?;
    let access_token = token_value
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("failed to fetch wecom access token".to_string()))?;
    let url =
        format!("https://qyapi.weixin.qq.com/cgi-bin/message/send?access_token={access_token}");
    let payload = json!({
        "touser": target_user,
        "msgtype": "text",
        "agentid": agent_id.parse::<i64>().unwrap_or(0),
        "text": { "content": content },
        "safe": 0
    });
    let res: Value = state
        .http
        .post(url)
        .json(&payload)
        .send()
        .await?
        .json()
        .await?;
    if res.get("errcode").and_then(Value::as_i64).unwrap_or(0) != 0 {
        return Err(AppError::BadRequest(format!("wecom send failed: {res}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::{
        cipher::{block_padding::NoPadding, BlockEncryptMut, KeyIvInit},
        Encryptor,
    };

    type Aes256CbcEnc = Encryptor<Aes256>;

    #[test]
    fn decrypt_payload_extracts_message_and_receive_id() {
        let aes_key = test_encoding_aes_key();
        let xml = "<xml><ToUserName><![CDATA[corp-123]]></ToUserName><FromUserName><![CDATA[kevin]]></FromUserName><MsgType><![CDATA[text]]></MsgType><Content><![CDATA[hello]]></Content><MsgId>42</MsgId></xml>";
        let encrypted = encrypt_payload(&aes_key, xml, "corp-123");

        let decrypted = decrypt_payload(&aes_key, &encrypted).expect("decrypt payload");

        assert_eq!(
            decrypted,
            WecomDecrypted {
                message: xml.to_string(),
                receive_id: "corp-123".to_string(),
            }
        );
    }

    #[test]
    fn decrypt_payload_accepts_real_wecom_key_shape() {
        let aes_key = "lFbb7s2MROtNqEWCZ4d8ZVyiQIEQO3HOJs4fDEoGdcD";
        let xml = "<xml><ToUserName><![CDATA[corp-123]]></ToUserName><FromUserName><![CDATA[kevin]]></FromUserName><MsgType><![CDATA[text]]></MsgType><Content><![CDATA[hello]]></Content><MsgId>42</MsgId></xml>";
        let encrypted = encrypt_payload(aes_key, xml, "corp-123");

        let decrypted = decrypt_payload(aes_key, &encrypted).expect("decrypt payload");

        assert_eq!(decrypted.receive_id, "corp-123");
        assert_eq!(decrypted.message, xml);
    }

    #[test]
    fn signature_uses_token_timestamp_nonce_and_ciphertext_sorted() {
        let signature = signature("token", "1700000000", "nonce", "encrypted");

        assert_eq!(signature, "a976d3f9651ff9c34c56c6f9774c36bca10ec1de");
    }

    #[test]
    fn unpad_rejects_inconsistent_padding() {
        let mut input = b"payload".to_vec();
        input.extend_from_slice(&[4, 4, 4, 3]);

        assert!(unpad(&input).is_err());
    }

    fn test_encoding_aes_key() -> String {
        let key = [7u8; 32];
        general_purpose::STANDARD
            .encode(key)
            .trim_end_matches('=')
            .to_string()
    }

    fn encrypt_payload(aes_key: &str, xml: &str, receive_id: &str) -> String {
        let key = WECOM_ENCODING_AES_KEY_ENGINE
            .decode(format!("{aes_key}="))
            .expect("valid aes key");
        let mut plaintext = Vec::new();
        plaintext.extend_from_slice(b"1234567890123456");
        plaintext.extend_from_slice(&(xml.len() as u32).to_be_bytes());
        plaintext.extend_from_slice(xml.as_bytes());
        plaintext.extend_from_slice(receive_id.as_bytes());
        let padding = 32 - plaintext.len() % 32;
        plaintext.extend(std::iter::repeat_n(padding as u8, padding));
        let len = plaintext.len();
        let encrypted = Aes256CbcEnc::new_from_slices(&key, &key[..16])
            .expect("valid cipher")
            .encrypt_padded_mut::<NoPadding>(&mut plaintext, len)
            .expect("encrypt payload");
        general_purpose::STANDARD.encode(encrypted)
    }
}
