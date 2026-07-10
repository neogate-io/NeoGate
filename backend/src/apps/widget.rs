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
    app_message_response, runtime::run_app_message, runtime_for_endpoint, AppMessageResponse,
    AppRuntime, IncomingAppMessage,
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
    if !anonymous_access_enabled(&runtime) {
        return Err(AppError::Forbidden(
            "anonymous access is disabled for this app".to_string(),
        ));
    }
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
    Ok(Json(app_message_response(outcome)))
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
    let title_json = serde_json::to_string(title)?;
    let theme_color = runtime
        .endpoint_config
        .get("theme_color")
        .and_then(Value::as_str)
        .filter(|value| is_hex_color(value))
        .unwrap_or("#176baf");
    let theme_color_json = serde_json::to_string(theme_color)?;
    let anonymous_access = anonymous_access_enabled(&runtime);
    let script = format!(
        r#"(function(){{
  var endpointId = {endpoint_id};
  var title = {title_json};
  var themeColor = {theme_color_json};
  var anonymousAccess = {anonymous_access};
  var root = document.createElement('div');
  root.id = 'neogate-widget-' + endpointId;
  root.style.cssText = 'position:fixed;right:20px;bottom:20px;z-index:2147483000;font-family:system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif';
  var launcher = document.createElement('button');
  launcher.type = 'button';
  launcher.setAttribute('aria-label', 'NeoGate chat');
  launcher.style.cssText = 'height:44px;border:0;border-radius:22px;background:' + themeColor + ';color:#fff;padding:0 16px;font-weight:650;box-shadow:0 8px 24px rgba(16,24,40,.18);cursor:pointer';
  launcher.textContent = title || 'NeoGate';
  root.appendChild(launcher);
  document.body.appendChild(root);
  var open = false;
  var panel;
  launcher.onclick = function(){{
    open = !open;
    if (!panel) {{
      panel = document.createElement('div');
      panel.style.cssText = 'position:absolute;right:0;bottom:56px;width:min(360px,calc(100vw - 32px));height:460px;background:#fff;border:1px solid #d7dee8;border-radius:8px;box-shadow:0 16px 40px rgba(16,24,40,.2);display:grid;grid-template-rows:1fr auto;overflow:hidden';
      panel.innerHTML = '<div data-log style="padding:14px;overflow:auto;font-size:14px;line-height:1.5;color:#101828"></div><form style="display:flex;gap:8px;padding:10px;border-top:1px solid #edf1f5"><input name="q" autocomplete="off" style="flex:1;border:1px solid #d7dee8;border-radius:6px;padding:9px;font-size:14px"/><button style="border:0;border-radius:6px;background:' + themeColor + ';color:#fff;padding:0 12px;font-weight:650">Send</button></form>';
      root.appendChild(panel);
      if (!anonymousAccess) {{
        panel.querySelector('[data-log]').textContent = '当前组件未开启匿名访问。';
        panel.querySelector('form').style.display = 'none';
        panel.style.gridTemplateRows = '1fr';
        return;
      }}
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
    let Some((origin_host, origin_host_port)) = origin_host(origin) else {
        return Err(AppError::Forbidden(
            "request origin is missing or invalid".to_string(),
        ));
    };
    let allowed = domains
        .iter()
        .filter_map(Value::as_str)
        .any(|domain| domain_matches(&origin_host, &origin_host_port, domain));
    allowed.then_some(()).ok_or(AppError::Forbidden(format!(
        "request origin '{origin}' is not allowed"
    )))
}

fn anonymous_access_enabled(runtime: &AppRuntime) -> bool {
    runtime
        .endpoint_config
        .get("anonymous_access")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

fn is_hex_color(value: &str) -> bool {
    let value = value.trim();
    let hex = value.strip_prefix('#').unwrap_or("");
    matches!(hex.len(), 3 | 6) && hex.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn origin_host(origin: &str) -> Option<(String, String)> {
    let value = origin.trim().to_ascii_lowercase();
    let without_scheme = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))?;
    let host_port = without_scheme.split('/').next()?.trim();
    if host_port.is_empty() {
        return None;
    }
    let host = host_port.split(':').next()?.trim();
    if host.is_empty() {
        return None;
    }
    Some((host.to_string(), host_port.to_string()))
}

fn domain_matches(origin_host: &str, origin_host_port: &str, domain: &str) -> bool {
    let domain = normalize_domain(domain);
    if domain.is_empty() {
        return false;
    }
    if domain.contains(':') {
        return origin_host_port == domain;
    }
    origin_host == domain || origin_host.ends_with(&format!(".{domain}"))
}

fn normalize_domain(domain: &str) -> String {
    let value = domain.trim().to_ascii_lowercase();
    let without_scheme = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .unwrap_or(&value);
    without_scheme
        .split('/')
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_domain_matching_allows_exact_and_subdomains() {
        let (host, host_port) = origin_host("https://docs.example.com:8443/page").unwrap();

        assert!(domain_matches(&host, &host_port, "example.com"));
        assert!(domain_matches(&host, &host_port, "docs.example.com:8443"));
        assert!(!domain_matches(&host, &host_port, "docs.example.com:9443"));
        assert!(!domain_matches(&host, &host_port, "evil-example.com"));
    }

    #[test]
    fn widget_color_validation_accepts_hex_colors_only() {
        assert!(is_hex_color("#176baf"));
        assert!(is_hex_color("#fff"));
        assert!(!is_hex_color("red"));
        assert!(!is_hex_color("url(javascript:alert(1))"));
    }
}
