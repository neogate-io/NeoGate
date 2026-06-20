use std::io::{Cursor, Read};

use axum::extract::Multipart;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::Row;
use zip::ZipArchive;

use crate::{
    error::{AppError, AppResult},
    id::DbId,
    input::trimmed_non_empty,
    AppState,
};

use super::openai::{
    credential_refresh_token, detect_openai_credential, refresh_openai_quota, refresh_openai_token,
    update_token_value, OPENAI_PROVIDER,
};
pub use super::openai::{
    openai_runtime_credential, openai_runtime_secret, OpenAiRuntimeCredential,
};

#[derive(Debug, Clone, Serialize)]
pub struct CredentialRecord {
    pub id: DbId,
    pub provider: String,
    pub identity_label: Option<String>,
    pub filename: String,
    pub enabled: bool,
    pub auth_mode: Option<String>,
    pub api_key_preview: Option<String>,
    pub has_oauth_tokens: bool,
    pub has_refresh_token: bool,
    pub has_id_token: bool,
    pub email: Option<String>,
    pub account_id: Option<String>,
    pub plan_type: Option<String>,
    pub last_refresh: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub quota: Option<CredentialQuota>,
    pub model_state_summary: CredentialModelStateSummary,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CredentialModelStateSummary {
    pub available: i64,
    pub unavailable: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CredentialModelRecord {
    pub credential_id: DbId,
    pub channel_endpoint_id: Option<DbId>,
    pub model: String,
    pub planned: bool,
    pub status: Option<String>,
    pub unavailable_until: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_status_code: Option<i32>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub success_count: i64,
    pub failure_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CredentialUploadResult {
    pub imported: Vec<CredentialRecord>,
    pub failed: Vec<CredentialUploadFailure>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CredentialUploadFailure {
    pub filename: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CredentialQuota {
    pub status: String,
    pub message: Option<String>,
    pub plan: Option<String>,
    pub five_hour: Option<QuotaWindow>,
    pub weekly: Option<QuotaWindow>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuotaWindow {
    pub percent: Option<f64>,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    pub reset_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedCredential {
    pub(super) provider: String,
    pub(super) identity_hash: String,
    pub(super) identity_label: Option<String>,
    pub(super) auth_mode: Option<String>,
    pub(super) api_key_preview: Option<String>,
    pub(super) has_oauth_tokens: bool,
    pub(super) has_refresh_token: bool,
    pub(super) has_id_token: bool,
    pub(super) email: Option<String>,
    pub(super) account_id: Option<String>,
    pub(super) plan_type: Option<String>,
    pub(super) last_refresh: Option<String>,
    pub(super) metadata: Value,
}

pub async fn list_credentials(
    state: &AppState,
    provider: Option<String>,
) -> AppResult<Vec<CredentialRecord>> {
    let provider = trimmed_non_empty(provider.as_deref()).map(str::to_string);

    let rows = sqlx::query(
        "SELECT id, provider, identity_label, filename, enabled, auth_mode,
                api_key_preview, has_oauth_tokens, has_refresh_token, has_id_token,
                email, account_id, plan_type, last_refresh, created_at, updated_at,
                COALESCE((SELECT COUNT(*) FROM credential_model cm
                          WHERE cm.credential_id = credential.id
                            AND cm.status = 'available'), 0)::BIGINT AS available_models,
                COALESCE((SELECT COUNT(*) FROM credential_model cm
                          WHERE cm.credential_id = credential.id
                            AND cm.status = 'unavailable'
                            AND (cm.unavailable_until IS NULL OR cm.unavailable_until > now())), 0)::BIGINT AS unavailable_models
         FROM credential
         WHERE ($1::TEXT IS NULL OR provider = $1)
         ORDER BY CASE WHEN $1::TEXT IS NULL THEN provider END ASC,
                  updated_at DESC,
                  id DESC",
    )
    .bind(provider)
    .fetch_all(&state.db.pool)
    .await?;

    rows.iter().map(credential_from_row).collect()
}

pub async fn upload_credentials(
    state: &AppState,
    mut multipart: Multipart,
) -> AppResult<CredentialUploadResult> {
    let mut result = CredentialUploadResult {
        imported: Vec::new(),
        failed: Vec::new(),
    };
    let mut saw_file = false;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| AppError::BadRequest(format!("invalid multipart upload: {err}")))?
    {
        if field.name() != Some("file") {
            continue;
        }
        saw_file = true;
        let filename = field
            .file_name()
            .map(str::to_string)
            .unwrap_or_else(|| "credential.json".to_string());
        let bytes = field
            .bytes()
            .await
            .map_err(|err| AppError::BadRequest(format!("failed to read uploaded file: {err}")))?;
        if bytes.len() > state.config.relay.credential_upload_limit_bytes {
            return Err(AppError::PayloadTooLarge(format!(
                "credential upload exceeds {} bytes",
                state.config.relay.credential_upload_limit_bytes
            )));
        }
        import_upload_bytes(state, &filename, &bytes, &mut result).await?;
    }

    if !saw_file {
        return Err(AppError::BadRequest(
            "multipart field file is required".to_string(),
        ));
    }

    Ok(result)
}

pub async fn refresh_credential(state: &AppState, id: DbId) -> AppResult<CredentialRecord> {
    let row = credential_secret_row(state, id).await?;
    let provider: String = row.try_get("provider")?;
    if provider != OPENAI_PROVIDER {
        return Err(AppError::BadRequest(format!(
            "unsupported credential refresh for provider: {provider}"
        )));
    }
    let content_ciphertext: String = row.try_get("content_ciphertext")?;
    let mut value: Value =
        serde_json::from_str(&state.secrets.plaintext(id, &content_ciphertext)?)?;
    let quota = refresh_openai_quota(state, id, &mut value).await;
    let parsed = detect_credential(state, &value)?;
    update_credential_content(state, id, &value, &parsed).await?;
    let mut record = get_credential(state, id).await?;
    record.quota = Some(quota);
    Ok(record)
}

pub async fn enable_credential(state: &AppState, id: DbId) -> AppResult<CredentialRecord> {
    set_credential_enabled(state, id, true).await
}

pub async fn disable_credential(state: &AppState, id: DbId) -> AppResult<CredentialRecord> {
    set_credential_enabled(state, id, false).await
}

pub async fn delete_credential(state: &AppState, id: DbId) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM credential WHERE id = $1")
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    state.secrets.forget(id);
    Ok(())
}

pub async fn runtime_secret_from_enabled_credential(
    state: &AppState,
    provider: &str,
) -> AppResult<String> {
    if provider != OPENAI_PROVIDER {
        return Err(AppError::BadRequest(
            "凭证文件目前仅支持 OpenAI；当前服务商请使用上游 API Key".to_string(),
        ));
    }

    let row = sqlx::query(
        "SELECT id, content_ciphertext
         FROM credential
         WHERE provider = $1 AND enabled = true
         ORDER BY updated_at DESC, id DESC
         LIMIT 1",
    )
    .bind(provider)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or_else(|| AppError::BadRequest("请先上传并启用 OpenAI 凭证文件".to_string()))?;

    let credential_id: DbId = row.try_get("id")?;
    let content_ciphertext: String = row.try_get("content_ciphertext")?;
    let value: Value = serde_json::from_str(
        &state
            .secrets
            .plaintext(credential_id, &content_ciphertext)?,
    )?;
    openai_runtime_secret(&value)
        .ok_or_else(|| AppError::BadRequest("凭证文件没有可用的 OpenAI token".to_string()))
}

pub async fn refresh_openai_runtime_credential(
    state: &AppState,
    id: DbId,
) -> AppResult<OpenAiRuntimeCredential> {
    let row = credential_secret_row(state, id).await?;
    let provider: String = row.try_get("provider")?;
    if provider != OPENAI_PROVIDER {
        return Err(AppError::BadRequest(format!(
            "unsupported credential refresh for provider: {provider}"
        )));
    }

    let content_ciphertext: String = row.try_get("content_ciphertext")?;
    let mut value: Value =
        serde_json::from_str(&state.secrets.plaintext(id, &content_ciphertext)?)?;
    let refresh_token = credential_refresh_token(&value).ok_or_else(|| {
        AppError::BadRequest("令牌文件已失效且缺少 refresh_token，请重新上传凭证".to_string())
    })?;
    let tokens = refresh_openai_token(state, &refresh_token)
        .await
        .map_err(|err| AppError::BadRequest(format!("令牌文件刷新失败，请重新上传凭证：{err}")))?;

    if let Some(token) = tokens.access_token {
        update_token_value(&mut value, "access_token", token);
    }
    if let Some(token) = tokens.refresh_token {
        update_token_value(&mut value, "refresh_token", token);
    }
    if let Some(token) = tokens.id_token {
        update_token_value(&mut value, "id_token", token);
    }
    if let Some(expires_in) = tokens.expires_in {
        let expires_at = Utc::now() + Duration::seconds(expires_in);
        update_token_value(&mut value, "expires_at", expires_at.timestamp().to_string());
    }
    value["last_refresh"] = Value::String(Utc::now().to_rfc3339());

    let parsed = detect_credential(state, &value)?;
    update_credential_content(state, id, &value, &parsed).await?;
    openai_runtime_credential(&value)
        .ok_or_else(|| AppError::BadRequest("刷新后凭证文件仍没有可用 OpenAI token".to_string()))
}

async fn import_upload_bytes(
    state: &AppState,
    filename: &str,
    bytes: &[u8],
    result: &mut CredentialUploadResult,
) -> AppResult<()> {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".zip") {
        import_zip(state, bytes, result).await;
        return Ok(());
    }
    if !lower.ends_with(".json") {
        result.failed.push(CredentialUploadFailure {
            filename: filename.to_string(),
            error: "only .json and .zip files are supported".to_string(),
        });
        return Ok(());
    }
    import_json_file(state, filename, bytes, result).await;
    Ok(())
}

async fn import_zip(state: &AppState, bytes: &[u8], result: &mut CredentialUploadResult) {
    let mut archive = match ZipArchive::new(Cursor::new(bytes)) {
        Ok(archive) => archive,
        Err(err) => {
            result.failed.push(CredentialUploadFailure {
                filename: "upload.zip".to_string(),
                error: format!("invalid zip file: {err}"),
            });
            return;
        }
    };

    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut file = match archive.by_index(index) {
            Ok(file) => file,
            Err(err) => {
                result.failed.push(CredentialUploadFailure {
                    filename: format!("zip entry #{index}"),
                    error: err.to_string(),
                });
                continue;
            }
        };
        if file.is_dir() {
            continue;
        }
        let Some(name) = file.enclosed_name().and_then(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        }) else {
            continue;
        };
        if !name.to_ascii_lowercase().ends_with(".json") {
            continue;
        }
        let mut content = Vec::new();
        if let Err(err) = file.read_to_end(&mut content) {
            result.failed.push(CredentialUploadFailure {
                filename: name,
                error: err.to_string(),
            });
            continue;
        }
        if content.len() > state.config.relay.credential_upload_limit_bytes {
            result.failed.push(CredentialUploadFailure {
                filename: name,
                error: format!(
                    "credential exceeds {} bytes",
                    state.config.relay.credential_upload_limit_bytes
                ),
            });
            continue;
        }
        entries.push((name, content));
    }

    for (name, content) in entries {
        import_json_file(state, &name, &content, result).await;
    }
}

async fn import_json_file(
    state: &AppState,
    filename: &str,
    bytes: &[u8],
    result: &mut CredentialUploadResult,
) {
    match import_json_file_inner(state, filename, bytes).await {
        Ok(record) => result.imported.push(record),
        Err(err) => result.failed.push(CredentialUploadFailure {
            filename: filename.to_string(),
            error: err.to_string(),
        }),
    }
}

async fn import_json_file_inner(
    state: &AppState,
    filename: &str,
    bytes: &[u8],
) -> AppResult<CredentialRecord> {
    let value: Value = serde_json::from_slice(bytes)?;
    let parsed = detect_credential(state, &value)?;
    let content = serde_json::to_string_pretty(&value)?;
    let content_ciphertext = state.secrets.encrypt(&content)?;
    let content_sha256 = hex::encode(Sha256::digest(content.as_bytes()));

    let row = sqlx::query(
        "INSERT INTO credential
         (provider, identity_hash, identity_label, filename, content_ciphertext,
          content_sha256, enabled, auth_mode, api_key_preview, has_oauth_tokens,
          has_refresh_token, has_id_token, email, account_id, plan_type, last_refresh, metadata)
         VALUES ($1, $2, $3, $4, $5, $6, TRUE, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
         ON CONFLICT (provider, identity_hash) DO UPDATE
         SET identity_label = EXCLUDED.identity_label,
             filename = EXCLUDED.filename,
             content_ciphertext = EXCLUDED.content_ciphertext,
             content_sha256 = EXCLUDED.content_sha256,
             auth_mode = EXCLUDED.auth_mode,
             api_key_preview = EXCLUDED.api_key_preview,
             has_oauth_tokens = EXCLUDED.has_oauth_tokens,
             has_refresh_token = EXCLUDED.has_refresh_token,
             has_id_token = EXCLUDED.has_id_token,
             email = EXCLUDED.email,
             account_id = EXCLUDED.account_id,
             plan_type = EXCLUDED.plan_type,
             last_refresh = EXCLUDED.last_refresh,
             metadata = EXCLUDED.metadata,
             updated_at = now()
         RETURNING id, provider, identity_label, filename, enabled, auth_mode,
                   api_key_preview, has_oauth_tokens, has_refresh_token, has_id_token,
                   email, account_id, plan_type, last_refresh, created_at, updated_at,
                   0::BIGINT AS available_models,
                   0::BIGINT AS unavailable_models",
    )
    .bind(&parsed.provider)
    .bind(&parsed.identity_hash)
    .bind(&parsed.identity_label)
    .bind(filename)
    .bind(content_ciphertext)
    .bind(content_sha256)
    .bind(&parsed.auth_mode)
    .bind(&parsed.api_key_preview)
    .bind(parsed.has_oauth_tokens)
    .bind(parsed.has_refresh_token)
    .bind(parsed.has_id_token)
    .bind(&parsed.email)
    .bind(&parsed.account_id)
    .bind(&parsed.plan_type)
    .bind(&parsed.last_refresh)
    .bind(sqlx::types::Json(parsed.metadata))
    .fetch_one(&state.db.pool)
    .await?;

    let id: DbId = row.try_get("id")?;
    state.secrets.forget(id);
    cleanup_credential_model_states(state, id, row.try_get("plan_type")?).await?;
    get_credential(state, id).await
}

fn detect_credential(state: &AppState, value: &Value) -> AppResult<ParsedCredential> {
    detect_openai_credential(state, value).ok_or_else(|| {
        AppError::BadRequest("unable to detect credential provider or stable identity".to_string())
    })
}

async fn get_credential(state: &AppState, id: DbId) -> AppResult<CredentialRecord> {
    let row = sqlx::query(
        "SELECT id, provider, identity_label, filename, enabled, auth_mode,
                api_key_preview, has_oauth_tokens, has_refresh_token, has_id_token,
                email, account_id, plan_type, last_refresh, created_at, updated_at,
                COALESCE((SELECT COUNT(*) FROM credential_model cm
                          WHERE cm.credential_id = credential.id
                            AND cm.status = 'available'), 0)::BIGINT AS available_models,
                COALESCE((SELECT COUNT(*) FROM credential_model cm
                          WHERE cm.credential_id = credential.id
                            AND cm.status = 'unavailable'
                            AND (cm.unavailable_until IS NULL OR cm.unavailable_until > now())), 0)::BIGINT AS unavailable_models
         FROM credential WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    credential_from_row(&row)
}

async fn set_credential_enabled(
    state: &AppState,
    id: DbId,
    enabled: bool,
) -> AppResult<CredentialRecord> {
    let row = sqlx::query(
        "UPDATE credential
         SET enabled = $2, updated_at = now()
         WHERE id = $1
         RETURNING id, provider, identity_label, filename, enabled, auth_mode,
                   api_key_preview, has_oauth_tokens, has_refresh_token, has_id_token,
                   email, account_id, plan_type, last_refresh, created_at, updated_at,
                   COALESCE((SELECT COUNT(*) FROM credential_model cm
                             WHERE cm.credential_id = credential.id
                               AND cm.status = 'available'), 0)::BIGINT AS available_models,
                   COALESCE((SELECT COUNT(*) FROM credential_model cm
                             WHERE cm.credential_id = credential.id
                               AND cm.status = 'unavailable'
                               AND (cm.unavailable_until IS NULL OR cm.unavailable_until > now())), 0)::BIGINT AS unavailable_models",
    )
    .bind(id)
    .bind(enabled)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    credential_from_row(&row)
}

async fn update_credential_content(
    state: &AppState,
    id: DbId,
    value: &Value,
    parsed: &ParsedCredential,
) -> AppResult<()> {
    let content = serde_json::to_string_pretty(value)?;
    let content_ciphertext = state.secrets.encrypt(&content)?;
    let content_sha256 = hex::encode(Sha256::digest(content.as_bytes()));
    sqlx::query(
        "UPDATE credential
         SET content_ciphertext = $2,
             content_sha256 = $3,
             identity_label = $4,
             auth_mode = $5,
             api_key_preview = $6,
             has_oauth_tokens = $7,
             has_refresh_token = $8,
             has_id_token = $9,
             email = $10,
             account_id = $11,
             plan_type = $12,
             last_refresh = $13,
             metadata = $14,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(content_ciphertext)
    .bind(content_sha256)
    .bind(&parsed.identity_label)
    .bind(&parsed.auth_mode)
    .bind(&parsed.api_key_preview)
    .bind(parsed.has_oauth_tokens)
    .bind(parsed.has_refresh_token)
    .bind(parsed.has_id_token)
    .bind(&parsed.email)
    .bind(&parsed.account_id)
    .bind(&parsed.plan_type)
    .bind(&parsed.last_refresh)
    .bind(sqlx::types::Json(parsed.metadata.clone()))
    .execute(&state.db.pool)
    .await?;
    cleanup_credential_model_states(state, id, parsed.plan_type.clone()).await?;
    state.secrets.forget(id);
    Ok(())
}

async fn credential_secret_row(state: &AppState, id: DbId) -> AppResult<sqlx::postgres::PgRow> {
    sqlx::query("SELECT id, provider, content_ciphertext FROM credential WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db.pool)
        .await?
        .ok_or(AppError::NotFound)
}

fn credential_from_row(row: &sqlx::postgres::PgRow) -> AppResult<CredentialRecord> {
    Ok(CredentialRecord {
        id: row.try_get("id")?,
        provider: row.try_get("provider")?,
        identity_label: row.try_get("identity_label")?,
        filename: row.try_get("filename")?,
        enabled: row.try_get("enabled")?,
        auth_mode: row.try_get("auth_mode")?,
        api_key_preview: row.try_get("api_key_preview")?,
        has_oauth_tokens: row.try_get("has_oauth_tokens")?,
        has_refresh_token: row.try_get("has_refresh_token")?,
        has_id_token: row.try_get("has_id_token")?,
        email: row.try_get("email")?,
        account_id: row.try_get("account_id")?,
        plan_type: row.try_get("plan_type")?,
        last_refresh: row.try_get("last_refresh")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        quota: None,
        model_state_summary: CredentialModelStateSummary {
            available: row.try_get("available_models")?,
            unavailable: row.try_get("unavailable_models")?,
        },
    })
}

fn credential_model_from_row(row: &sqlx::postgres::PgRow) -> AppResult<CredentialModelRecord> {
    Ok(CredentialModelRecord {
        credential_id: row.try_get("credential_id")?,
        channel_endpoint_id: row.try_get("channel_endpoint_id")?,
        model: row.try_get("model")?,
        planned: row.try_get("planned")?,
        status: row.try_get("status")?,
        unavailable_until: row.try_get("unavailable_until")?,
        last_error: row.try_get("last_error")?,
        last_status_code: row.try_get("last_status_code")?,
        last_seen_at: row.try_get("last_seen_at")?,
        success_count: row.try_get("success_count")?,
        failure_count: row.try_get("failure_count")?,
    })
}

pub async fn list_credential_models(
    state: &AppState,
    id: DbId,
) -> AppResult<Vec<CredentialModelRecord>> {
    let credential = sqlx::query(
        "SELECT provider, plan_type
         FROM credential
         WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let provider: String = credential.try_get("provider")?;
    let plan_type: Option<String> = credential.try_get("plan_type")?;

    let rows = sqlx::query(
        "WITH planned AS (
             SELECT pp.model
             FROM provider_plan pp
             WHERE pp.provider = $2
               AND pp.enabled = TRUE
               AND ($3::TEXT IS NOT NULL AND pp.plan_type = $3)
         ),
         actual AS (
             SELECT cm.channel_endpoint_id, cm.model, cm.status,
                    cm.unavailable_until, cm.last_error, cm.last_status_code,
                    cm.last_seen_at, cm.success_count, cm.failure_count
             FROM credential_model cm
             WHERE cm.credential_id = $1
         )
         SELECT $1::BIGINT AS credential_id,
                actual.channel_endpoint_id,
                COALESCE(planned.model, actual.model) AS model,
                (planned.model IS NOT NULL) AS planned,
                actual.status,
                actual.unavailable_until,
                actual.last_error,
                actual.last_status_code,
                actual.last_seen_at,
                COALESCE(actual.success_count, 0)::BIGINT AS success_count,
                COALESCE(actual.failure_count, 0)::BIGINT AS failure_count
         FROM planned
         FULL OUTER JOIN actual ON actual.model = planned.model
         ORDER BY planned DESC, model ASC, channel_endpoint_id ASC NULLS FIRST",
    )
    .bind(id)
    .bind(provider)
    .bind(plan_type)
    .fetch_all(&state.db.pool)
    .await?;

    rows.iter().map(credential_model_from_row).collect()
}

pub async fn reset_credential_model(state: &AppState, id: DbId, model: &str) -> AppResult<()> {
    let model = model.trim();
    if model.is_empty() {
        return Err(AppError::BadRequest("model is required".to_string()));
    }
    let result = sqlx::query(
        "DELETE FROM credential_model
         WHERE credential_id = $1
           AND model = $2",
    )
    .bind(id)
    .bind(model)
    .execute(&state.db.pool)
    .await?;
    if result.rows_affected() == 0 {
        let exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM credential WHERE id = $1")
            .bind(id)
            .fetch_one(&state.db.pool)
            .await?;
        if exists == 0 {
            return Err(AppError::NotFound);
        }
    }
    Ok(())
}

async fn cleanup_credential_model_states(
    state: &AppState,
    id: DbId,
    plan_type: Option<String>,
) -> AppResult<()> {
    sqlx::query(
        "DELETE FROM credential_model
         WHERE credential_id = $1
           AND (
               unavailable_until <= now()
               OR (
                   $2::TEXT IS NOT NULL
                   AND EXISTS (
                       SELECT 1
                       FROM credential cr
                       JOIN channel_endpoint ce
                         ON ce.id = credential_model.channel_endpoint_id
                       JOIN provider_plan pp
                         ON pp.provider = cr.provider
                        AND pp.protocol = ce.protocol
                        AND pp.plan_type = $2
                        AND pp.enabled = TRUE
                       WHERE cr.id = $1
                   )
                   AND NOT EXISTS (
                       SELECT 1
                       FROM credential cr
                       JOIN channel_endpoint ce
                         ON ce.id = credential_model.channel_endpoint_id
                       JOIN provider_plan pp
                         ON pp.provider = cr.provider
                        AND pp.protocol = ce.protocol
                        AND pp.plan_type = $2
                        AND pp.model = credential_model.model
                        AND pp.enabled = TRUE
                       WHERE cr.id = $1
                   )
               )
           )",
    )
    .bind(id)
    .bind(plan_type)
    .execute(&state.db.pool)
    .await?;
    Ok(())
}
