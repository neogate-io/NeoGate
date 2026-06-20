use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

use super::{
    constant_time_eq, hmac_sha256_hex, runtime::run_app_message, runtime_for_endpoint,
    secret_plaintext, AppMessageResponse, AppRuntime, IncomingAppMessage,
};

const SECRET_KEY: &str = "secret";

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct MessageRequest {
    external_user_id: Option<String>,
    external_conversation_id: Option<String>,
    message_id: Option<String>,
    content: String,
    metadata: Option<serde_json::Value>,
    trace_id: Option<String>,
}

pub(super) async fn message(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<DbId>,
    headers: HeaderMap,
    Json(req): Json<MessageRequest>,
) -> AppResult<Json<AppMessageResponse>> {
    let runtime = runtime_for_endpoint(&state, endpoint_id, "webhook").await?;
    let body = serde_json::to_vec(&req)?;
    verify_signature(&state, &runtime, &headers, &body)?;
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

fn verify_signature(
    state: &AppState,
    runtime: &AppRuntime,
    headers: &HeaderMap,
    body: &[u8],
) -> AppResult<()> {
    let secret = secret_plaintext(state, runtime, SECRET_KEY)?;
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
