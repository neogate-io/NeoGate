use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

use super::{
    runtime::run_app_message, runtime_for_endpoint, AppMessageResponse, AppRuntime,
    IncomingAppMessage,
};

#[derive(Debug, Deserialize)]
pub(super) struct MessageRequest {
    session_id: Option<String>,
    content: String,
    metadata: Option<Value>,
}

pub(super) async fn message(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<DbId>,
    headers: HeaderMap,
    Json(req): Json<MessageRequest>,
) -> AppResult<Json<AppMessageResponse>> {
    let runtime = runtime_for_endpoint(&state, endpoint_id, "widget").await?;
    verify_origin(&runtime, &headers)?;
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

pub(super) async fn script(
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

fn verify_origin(runtime: &AppRuntime, headers: &HeaderMap) -> AppResult<()> {
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
