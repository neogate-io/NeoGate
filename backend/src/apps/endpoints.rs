use std::sync::Arc;

use aes::Aes256;
use axum::{
    body::{to_bytes, Body},
    extract::{Path, Query, State},
    http::{header, HeaderMap},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose, Engine as _};
use cbc::{
    cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit},
    Decryptor,
};
use serde_json::{json, Value};
use sha1::{Digest as Sha1Digest, Sha1};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

use super::{constant_time_eq, extract_xml_value, hmac_sha256_hex};
use super::{
    runtime::run_app_message, runtime_for_endpoint, secret_plaintext, AppMessageResponse,
    AppRuntime, IncomingAppMessage, WebhookMessageRequest, WecomCallbackQuery,
    WidgetMessageRequest, APP_BODY_LIMIT_BYTES, WEBHOOK_SECRET_KEY, WECOM_AES_SECRET_KEY,
    WECOM_CORP_SECRET_KEY, WECOM_TOKEN_SECRET_KEY,
};

type Aes256CbcDec = Decryptor<Aes256>;

pub(super) async fn webhook_message(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<DbId>,
    headers: HeaderMap,
    Json(req): Json<WebhookMessageRequest>,
) -> AppResult<Json<AppMessageResponse>> {
    let runtime = runtime_for_endpoint(&state, endpoint_id, "webhook").await?;
    let body = serde_json::to_vec(&req)?;
    verify_webhook_signature(&state, &runtime, &headers, &body)?;
    let message = IncomingAppMessage {
        external_user_id: req
            .external_user_id
            .unwrap_or_else(|| "webhook".to_string()),
        external_conversation_id: req
            .external_conversation_id
            .unwrap_or_else(|| "default".to_string()),
        external_message_id: req.message_id,
        content: req.content,
        metadata: req.metadata.unwrap_or_else(|| json!({})),
        trace_id: req.trace_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
    };
    let outcome = run_app_message(Arc::clone(&state), runtime, message).await?;
    Ok(Json(AppMessageResponse {
        ok: true,
        conversation_id: outcome.conversation_id,
        message: outcome.message,
        trace_id: outcome.trace_id,
        duplicate: outcome.duplicate,
    }))
}

pub(super) async fn widget_message(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<DbId>,
    headers: HeaderMap,
    Json(req): Json<WidgetMessageRequest>,
) -> AppResult<Json<AppMessageResponse>> {
    let runtime = runtime_for_endpoint(&state, endpoint_id, "widget").await?;
    verify_widget_origin(&runtime, &headers)?;
    let session_id = req.session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let trace_id = Uuid::new_v4().to_string();
    let message = IncomingAppMessage {
        external_user_id: session_id.clone(),
        external_conversation_id: session_id,
        external_message_id: Some(trace_id.clone()),
        content: req.content,
        metadata: req.metadata.unwrap_or_else(|| json!({})),
        trace_id,
    };
    let outcome = run_app_message(Arc::clone(&state), runtime, message).await?;
    Ok(Json(AppMessageResponse {
        ok: true,
        conversation_id: outcome.conversation_id,
        message: outcome.message,
        trace_id: outcome.trace_id,
        duplicate: outcome.duplicate,
    }))
}

pub(super) async fn widget_script(
    State(state): State<Arc<AppState>>,
    Path(script_name): Path<String>,
) -> AppResult<Response> {
    let endpoint_id = script_name
        .strip_suffix(".js")
        .ok_or(AppError::NotFound)?
        .parse::<DbId>()
        .map_err(|_| AppError::NotFound)?;
    let runtime = runtime_for_endpoint(&state, endpoint_id, "widget").await?;
    let title = runtime
        .endpoint_config
        .get("welcome")
        .and_then(Value::as_str)
        .unwrap_or(&runtime.name);
    let script = format!(
        r#"(function(){{
  var endpointId = {endpoint_id};
  var root = document.createElement('div');
  root.id = 'neogate-widget-' + endpointId;
  root.style.cssText = 'position:fixed;right:20px;bottom:20px;z-index:2147483000;font-family:system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif';
  root.innerHTML = '<button type="button" aria-label="NeoGate chat" style="height:44px;border:0;border-radius:22px;background:#176baf;color:#fff;padding:0 16px;font-weight:650;box-shadow:0 8px 24px rgba(16,24,40,.18);cursor:pointer">{title}</button>';
  document.body.appendChild(root);
  var open = false;
  var panel;
  root.querySelector('button').onclick = function(){{
    open = !open;
    if (!panel) {{
      panel = document.createElement('div');
      panel.style.cssText = 'position:absolute;right:0;bottom:56px;width:min(360px,calc(100vw - 32px));height:460px;background:#fff;border:1px solid #d7dee8;border-radius:8px;box-shadow:0 16px 40px rgba(16,24,40,.2);display:grid;grid-template-rows:1fr auto;overflow:hidden';
      panel.innerHTML = '<div data-log style="padding:14px;overflow:auto;font-size:14px;line-height:1.5;color:#101828"></div><form style="display:flex;gap:8px;padding:10px;border-top:1px solid #edf1f5"><input name="q" autocomplete="off" style="flex:1;border:1px solid #d7dee8;border-radius:6px;padding:9px;font-size:14px"/><button style="border:0;border-radius:6px;background:#176baf;color:#fff;padding:0 12px;font-weight:650">Send</button></form>';
      root.appendChild(panel);
      var session = localStorage.getItem('neogate_widget_' + endpointId) || crypto.randomUUID();
      localStorage.setItem('neogate_widget_' + endpointId, session);
      panel.querySelector('form').onsubmit = async function(e){{
        e.preventDefault();
        var input = panel.querySelector('input');
        var text = input.value.trim();
        if (!text) return;
        input.value = '';
        var log = panel.querySelector('[data-log]');
        log.innerHTML += '<div><strong>You</strong><br>' + escapeHtml(text) + '</div>';
        var res = await fetch('/apps/widget/' + endpointId + '/messages', {{method:'POST',headers:{{'content-type':'application/json'}},body:JSON.stringify({{session_id:session,content:text}})}});
        var data = await res.json().catch(function(){{return {{message:'Request failed'}}}});
        log.innerHTML += '<div style="margin-top:12px"><strong>AI</strong><br>' + escapeHtml(data.message || 'Request failed') + '</div>';
        log.scrollTop = log.scrollHeight;
      }};
    }}
    panel.style.display = open ? 'grid' : 'none';
  }};
  function escapeHtml(s){{return String(s).replace(/[&<>"']/g,function(c){{return {{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}}[c]}})}}
}})();"#,
    );
    Ok((
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        script,
    )
        .into_response())
}

pub(super) async fn wecom_verify(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<DbId>,
    Query(query): Query<WecomCallbackQuery>,
) -> AppResult<Response> {
    let runtime = runtime_for_endpoint(&state, endpoint_id, "wecom").await?;
    let msg_signature = query
        .msg_signature
        .as_deref()
        .ok_or(AppError::Unauthorized)?;
    let timestamp = query.timestamp.as_deref().ok_or(AppError::Unauthorized)?;
    let nonce = query.nonce.as_deref().ok_or(AppError::Unauthorized)?;
    let echostr = query.echostr.as_deref().ok_or(AppError::Unauthorized)?;
    verify_wecom_signature(&state, &runtime, msg_signature, timestamp, nonce, echostr)?;
    let plaintext = decrypt_wecom(&state, &runtime, echostr)?;
    Ok(plaintext.into_response())
}

pub(super) async fn wecom_message(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<DbId>,
    Query(query): Query<WecomCallbackQuery>,
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
    verify_wecom_signature(
        &state,
        &runtime,
        msg_signature,
        timestamp,
        nonce,
        &encrypted,
    )?;
    let plaintext = decrypt_wecom(&state, &runtime, &encrypted)?;
    let content = extract_xml_value(plaintext.as_bytes(), "Content").unwrap_or_default();
    let msg_type = extract_xml_value(plaintext.as_bytes(), "MsgType").unwrap_or_default();
    let from_user = extract_xml_value(plaintext.as_bytes(), "FromUserName")
        .unwrap_or_else(|| "wecom".to_string());
    let msg_id = extract_xml_value(plaintext.as_bytes(), "MsgId");
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
                    send_wecom_text(&state_clone, &runtime, &target_user, &outcome.message).await
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

fn verify_webhook_signature(
    state: &AppState,
    runtime: &AppRuntime,
    headers: &HeaderMap,
    body: &[u8],
) -> AppResult<()> {
    let secret = secret_plaintext(state, runtime, WEBHOOK_SECRET_KEY)?;
    if secret.is_empty() {
        return Ok(());
    }
    let signature = headers
        .get("x-neogate-signature")
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Unauthorized)?;
    let expected = hmac_sha256_hex(secret.as_bytes(), body);
    constant_time_eq(signature.as_bytes(), expected.as_bytes())
        .then_some(())
        .ok_or(AppError::Unauthorized)
}

fn verify_widget_origin(runtime: &AppRuntime, headers: &HeaderMap) -> AppResult<()> {
    let Some(domains) = runtime
        .endpoint_config
        .get("allowed_domains")
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    if domains.is_empty() {
        return Ok(());
    }
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let allowed = domains
        .iter()
        .filter_map(Value::as_str)
        .any(|domain| !domain.trim().is_empty() && origin.contains(domain.trim()));
    allowed.then_some(()).ok_or(AppError::Forbidden)
}

fn verify_wecom_signature(
    state: &AppState,
    runtime: &AppRuntime,
    msg_signature: &str,
    timestamp: &str,
    nonce: &str,
    encrypted: &str,
) -> AppResult<()> {
    let token = secret_plaintext(state, runtime, WECOM_TOKEN_SECRET_KEY)?;
    let mut parts = vec![
        token,
        timestamp.to_string(),
        nonce.to_string(),
        encrypted.to_string(),
    ];
    parts.sort();
    let mut hasher = Sha1::new();
    for part in parts {
        hasher.update(part.as_bytes());
    }
    let expected = hex::encode(hasher.finalize());
    constant_time_eq(msg_signature.as_bytes(), expected.as_bytes())
        .then_some(())
        .ok_or(AppError::Unauthorized)
}

fn decrypt_wecom(state: &AppState, runtime: &AppRuntime, encrypted: &str) -> AppResult<String> {
    let aes_key = secret_plaintext(state, runtime, WECOM_AES_SECRET_KEY)?;
    let key = general_purpose::STANDARD
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
    let plaintext = wecom_unpad(decrypted)?;
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
    String::from_utf8(plaintext[start..end].to_vec())
        .map_err(|_| AppError::BadRequest("invalid decrypted utf8".to_string()))
}

fn wecom_unpad(input: &[u8]) -> AppResult<&[u8]> {
    let Some(&pad) = input.last() else {
        return Err(AppError::BadRequest("empty decrypted payload".to_string()));
    };
    let pad = if pad == 0 { 32 } else { pad as usize };
    if pad > 32 || pad > input.len() {
        return Err(AppError::BadRequest(
            "invalid decrypted padding".to_string(),
        ));
    }
    Ok(&input[..input.len() - pad])
}

async fn send_wecom_text(
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
    let secret = secret_plaintext(state, runtime, WECOM_CORP_SECRET_KEY)?;
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
