use chrono::{DateTime, Utc};
use lettre::message::Mailbox;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    config::RuntimeProbe,
    email::{smtp_test_error, EmailConfig, EmailService, SMTP_SETTING_KEY},
    error::{AppError, AppResult},
    id::DbId,
    input::trimmed_non_empty_owned,
    setup::bootstrap::{save_public_base_url_config, validate_public_base_url},
    AppState,
};

const SITE_BRAND_SETTING_KEY: &str = "site_brand";
pub const ADMIN_MODEL_SETTING_KEY: &str = "admin_model";
#[derive(Debug, Serialize)]
pub struct SiteSettingRecord {
    pub site_name: String,
    pub public_base_url: Option<String>,
    pub logo_url: Option<String>,
    pub billing_currency: String,
    pub env_write_supported: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpsertSiteSettingRequest {
    pub site_name: String,
    pub public_base_url: String,
    pub logo_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpsertSiteSettingResponse {
    pub ok: bool,
    pub restart_required: bool,
    pub setting: SiteSettingRecord,
}

#[derive(Debug, Serialize)]
pub struct SmtpSettingRecord {
    pub configured: bool,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: Option<String>,
    pub smtp_password_set: bool,
    pub smtp_tls: bool,
    pub from_email: String,
    pub from_name: Option<String>,
    pub subject_prefix: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertSmtpSettingRequest {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: Option<String>,
    pub smtp_password: Option<String>,
    #[serde(default)]
    pub clear_smtp_password: bool,
    pub smtp_tls: bool,
    pub from_email: String,
    pub from_name: Option<String>,
    pub subject_prefix: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TestSmtpSettingResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdminModelSettingRecord {
    pub default_text_model: Option<String>,
    pub default_text_channel_id: Option<DbId>,
    pub default_text_channel_name: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertAdminModelSettingRequest {
    pub default_text_model: Option<String>,
    pub default_text_channel_id: Option<DbId>,
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredSmtpSetting {
    smtp_host: String,
    smtp_port: u16,
    smtp_username: Option<String>,
    password_ciphertext: Option<String>,
    smtp_tls: bool,
    from_email: String,
    from_name: Option<String>,
    subject_prefix: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredSiteBrandSetting {
    site_name: Option<String>,
    logo_url: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredAdminModelSetting {
    default_text_model: Option<String>,
    default_text_channel_id: Option<DbId>,
}

pub async fn get_site_setting(state: &AppState) -> AppResult<SiteSettingRecord> {
    let probe = RuntimeProbe::from_env()?;
    let env_write_supported = !probe.runtime_mode.is_distributed();
    let brand = existing_site_brand_setting(state).await?;
    let site_name = brand
        .as_ref()
        .and_then(|setting| setting.site_name.clone())
        .or(probe.site_name)
        .unwrap_or_else(|| "NeoGate".to_string());
    let logo_url = brand.and_then(|setting| setting.logo_url);
    Ok(SiteSettingRecord {
        site_name,
        public_base_url: probe.public_base_url,
        logo_url,
        billing_currency: state.config.billing_currency.as_str().to_string(),
        env_write_supported,
    })
}

pub async fn upsert_site_setting(
    state: &AppState,
    req: UpsertSiteSettingRequest,
) -> AppResult<UpsertSiteSettingResponse> {
    let probe = RuntimeProbe::from_env()?;
    let site_name = required_trimmed(req.site_name, "SITE_NAME is required")?;
    let public_base_url = required_trimmed(req.public_base_url, "PUBLIC_BASE_URL is required")?
        .trim_end_matches('/')
        .to_string();
    validate_public_base_url(&public_base_url)?;
    let logo_url = normalize_logo_url(req.logo_url)?;
    upsert_site_brand_setting(state, site_name, logo_url).await?;

    let env_changed = probe.public_base_url.as_deref() != Some(public_base_url.as_str());
    if env_changed {
        save_public_base_url_config(public_base_url).await?;
    }

    Ok(UpsertSiteSettingResponse {
        ok: true,
        restart_required: env_changed,
        setting: get_site_setting(state).await?,
    })
}

async fn existing_site_brand_setting(
    state: &AppState,
) -> AppResult<Option<StoredSiteBrandSetting>> {
    let Some(row) = sqlx::query("SELECT value FROM setting WHERE key = $1")
        .bind(SITE_BRAND_SETTING_KEY)
        .fetch_optional(&state.db.pool)
        .await?
    else {
        return Ok(None);
    };
    let value: serde_json::Value = row.try_get("value")?;
    Ok(Some(serde_json::from_value(value)?))
}

async fn upsert_site_brand_setting(
    state: &AppState,
    site_name: String,
    logo_url: Option<String>,
) -> AppResult<()> {
    let value = serde_json::to_value(StoredSiteBrandSetting {
        site_name: Some(site_name),
        logo_url,
    })?;
    sqlx::query(
        r#"
        INSERT INTO setting (key, value)
        VALUES ($1, $2)
        ON CONFLICT (key)
        DO UPDATE SET value = EXCLUDED.value, updated_at = now()
        "#,
    )
    .bind(SITE_BRAND_SETTING_KEY)
    .bind(value)
    .execute(&state.db.pool)
    .await?;
    Ok(())
}

fn normalize_logo_url(value: Option<String>) -> AppResult<Option<String>> {
    let Some(value) = optional_trimmed(value) else {
        return Ok(None);
    };
    if value.starts_with("http://") || value.starts_with("https://") {
        return Ok(Some(value));
    }
    Err(AppError::BadRequest(
        "logo URL must be a complete http(s) URL".to_string(),
    ))
}

pub async fn get_admin_model_setting(state: &AppState) -> AppResult<AdminModelSettingRecord> {
    let Some(row) = sqlx::query("SELECT value, updated_at FROM setting WHERE key = $1")
        .bind(ADMIN_MODEL_SETTING_KEY)
        .fetch_optional(&state.db.pool)
        .await?
    else {
        return Ok(AdminModelSettingRecord {
            default_text_model: None,
            default_text_channel_id: None,
            default_text_channel_name: None,
            updated_at: None,
        });
    };
    let value: serde_json::Value = row.try_get("value")?;
    let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
    let setting: StoredAdminModelSetting = serde_json::from_value(value)?;
    admin_model_record_from_stored(state, setting, Some(updated_at)).await
}

pub async fn upsert_admin_model_setting(
    state: &AppState,
    req: UpsertAdminModelSettingRequest,
) -> AppResult<AdminModelSettingRecord> {
    let model = req
        .default_text_model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    match (&model, req.default_text_channel_id) {
        (Some(model), Some(channel_id)) => {
            ensure_callable_text_model(state, channel_id, model).await?
        }
        (None, None) => {}
        _ => {
            return Err(AppError::BadRequest(
                "default text model and channel must be set together".to_string(),
            ))
        }
    }
    let value = serde_json::to_value(StoredAdminModelSetting {
        default_text_model: model,
        default_text_channel_id: req.default_text_channel_id,
    })?;
    let row = sqlx::query(
        r#"
        INSERT INTO setting (key, value)
        VALUES ($1, $2)
        ON CONFLICT (key)
        DO UPDATE SET value = EXCLUDED.value, updated_at = now()
        RETURNING value, updated_at
        "#,
    )
    .bind(ADMIN_MODEL_SETTING_KEY)
    .bind(value)
    .fetch_one(&state.db.pool)
    .await?;
    let value: serde_json::Value = row.try_get("value")?;
    let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
    let setting: StoredAdminModelSetting = serde_json::from_value(value)?;
    admin_model_record_from_stored(state, setting, Some(updated_at)).await
}

pub async fn ensure_default_text_model_setting(state: &AppState) -> AppResult<()> {
    let setting = get_admin_model_setting(state).await?;
    if let (Some(model), Some(channel_id)) = (
        setting.default_text_model.as_deref(),
        setting.default_text_channel_id,
    ) {
        if ensure_callable_text_model(state, channel_id, model)
            .await
            .is_ok()
        {
            tracing::debug!(
                model,
                channel_id,
                "admin default text model setting is already callable"
            );
            return Ok(());
        }
        tracing::info!(
            model,
            channel_id,
            "admin default text model setting is not callable; searching replacement"
        );
    }
    let Some(candidate) = first_callable_text_model(state).await? else {
        log_default_text_model_candidate_summary(state).await?;
        return Ok(());
    };
    let model = candidate.model;
    let channel_id = candidate.channel_id;
    let value = serde_json::to_value(StoredAdminModelSetting {
        default_text_model: Some(model.clone()),
        default_text_channel_id: Some(channel_id),
    })?;
    sqlx::query(
        r#"
        INSERT INTO setting (key, value)
        VALUES ($1, $2)
        ON CONFLICT (key)
        DO UPDATE SET value = EXCLUDED.value, updated_at = now()
        "#,
    )
    .bind(ADMIN_MODEL_SETTING_KEY)
    .bind(value)
    .execute(&state.db.pool)
    .await?;
    tracing::info!(
        model,
        channel_id,
        "admin default text model setting initialized"
    );
    Ok(())
}

pub async fn resolve_default_text_model(
    state: &AppState,
) -> AppResult<Option<(String, DbId, String)>> {
    let setting = get_admin_model_setting(state).await?;
    let Some(model) = setting.default_text_model else {
        return Ok(None);
    };
    let Some(channel_id) = setting.default_text_channel_id else {
        return Ok(None);
    };
    ensure_callable_text_model(state, channel_id, &model).await?;
    Ok(Some((
        model,
        channel_id,
        setting.default_text_channel_name.unwrap_or_default(),
    )))
}

struct TextModelCandidate {
    model: String,
    channel_id: DbId,
}

async fn first_callable_text_model(state: &AppState) -> AppResult<Option<TextModelCandidate>> {
    let rows = sqlx::query(
        r#"
        SELECT cm.model, c.id AS channel_id
        FROM channel_model cm
        JOIN channel c ON c.id = cm.channel_id
        JOIN provider p ON p.code = c.provider
        WHERE p.enabled = TRUE
          AND c.enabled = TRUE
          AND cm.enabled = TRUE
          AND cm.status = 'available'
          AND (
              cm.runtime_status = 'normal'
              OR (cm.runtime_status = 'cooldown' AND cm.cooldown_until <= now())
          )
          AND EXISTS (
              SELECT 1 FROM channel_endpoint ce
              WHERE ce.channel_id = c.id
                AND ce.enabled = TRUE
                AND ce.protocol IN ('openai', 'anthropic')
                AND (
                    EXISTS (
                        SELECT 1 FROM unnest(ce.models) AS endpoint_model(model)
                        WHERE btrim(endpoint_model.model) = cm.model
                    )
                    OR NOT EXISTS (
                        SELECT 1 FROM unnest(ce.models) AS endpoint_model(model)
                        WHERE btrim(endpoint_model.model) <> ''
                    )
                )
          )
          AND (
              (
                  c.use_credentials = FALSE
                  AND EXISTS (
                      SELECT 1 FROM channel_key ck
                      WHERE ck.channel_id = c.id
                        AND ck.enabled = TRUE
                  )
              )
              OR (
                  c.use_credentials = TRUE
                  AND EXISTS (
                      SELECT 1 FROM credential cr
                      WHERE cr.provider = c.provider
                        AND cr.enabled = TRUE
                  )
              )
          )
        ORDER BY c.priority DESC, cm.updated_at DESC, cm.model ASC
        LIMIT 1
        "#,
    )
    .fetch_all(&state.db.pool)
    .await?;
    rows.into_iter()
        .next()
        .map(|row| {
            Ok(TextModelCandidate {
                model: row.try_get("model")?,
                channel_id: row.try_get("channel_id")?,
            })
        })
        .transpose()
}

async fn ensure_callable_text_model(
    state: &AppState,
    channel_id: DbId,
    model: &str,
) -> AppResult<()> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM channel_model cm
            JOIN channel c ON c.id = cm.channel_id
            JOIN provider p ON p.code = c.provider
            WHERE c.id = $1
              AND cm.model = $2
              AND p.enabled = TRUE
              AND c.enabled = TRUE
              AND cm.enabled = TRUE
              AND cm.status = 'available'
              AND (
                  cm.runtime_status = 'normal'
                  OR (cm.runtime_status = 'cooldown' AND cm.cooldown_until <= now())
              )
              AND EXISTS (
                  SELECT 1 FROM channel_endpoint ce
                  WHERE ce.channel_id = c.id
                    AND ce.enabled = TRUE
                    AND ce.protocol IN ('openai', 'anthropic')
                    AND (
                        EXISTS (
                            SELECT 1 FROM unnest(ce.models) AS endpoint_model(model)
                            WHERE btrim(endpoint_model.model) = cm.model
                        )
                        OR NOT EXISTS (
                            SELECT 1 FROM unnest(ce.models) AS endpoint_model(model)
                            WHERE btrim(endpoint_model.model) <> ''
                        )
                    )
              )
              AND (
                  (
                      c.use_credentials = FALSE
                      AND EXISTS (
                          SELECT 1 FROM channel_key ck
                          WHERE ck.channel_id = c.id
                            AND ck.enabled = TRUE
                      )
                  )
                  OR (
                      c.use_credentials = TRUE
                      AND EXISTS (
                          SELECT 1 FROM credential cr
                          WHERE cr.provider = c.provider
                            AND cr.enabled = TRUE
                      )
                  )
              )
        )
        "#,
    )
    .bind(channel_id)
    .bind(model)
    .fetch_one(&state.db.pool)
    .await?;
    if !exists {
        return Err(AppError::BadRequest(
            "default text model is not currently callable".to_string(),
        ));
    }
    Ok(())
}

async fn log_default_text_model_candidate_summary(state: &AppState) -> AppResult<()> {
    let row = sqlx::query(
        r#"
        WITH base AS (
            SELECT cm.id, cm.model, c.id AS channel_id, c.use_credentials,
                   p.enabled AS provider_enabled,
                   c.enabled AS channel_enabled,
                   cm.enabled AS model_enabled,
                   cm.status AS model_status,
                   cm.runtime_status,
                   cm.cooldown_until
            FROM channel_model cm
            JOIN channel c ON c.id = cm.channel_id
            JOIN provider p ON p.code = c.provider
        ),
        eligible_model AS (
            SELECT *
            FROM base
            WHERE provider_enabled = TRUE
              AND channel_enabled = TRUE
              AND model_enabled = TRUE
              AND model_status = 'available'
              AND (
                  runtime_status = 'normal'
                  OR (runtime_status = 'cooldown' AND cooldown_until <= now())
              )
        ),
        endpoint_ready AS (
            SELECT em.*
            FROM eligible_model em
            WHERE EXISTS (
                SELECT 1 FROM channel_endpoint ce
                WHERE ce.channel_id = em.channel_id
                  AND ce.enabled = TRUE
                  AND ce.protocol IN ('openai', 'anthropic')
                  AND (
                      EXISTS (
                          SELECT 1 FROM unnest(ce.models) AS endpoint_model(model)
                          WHERE btrim(endpoint_model.model) = em.model
                      )
                      OR NOT EXISTS (
                          SELECT 1 FROM unnest(ce.models) AS endpoint_model(model)
                          WHERE btrim(endpoint_model.model) <> ''
                      )
                  )
            )
        ),
        secret_ready AS (
            SELECT er.*
            FROM endpoint_ready er
            WHERE (
                er.use_credentials = FALSE
                AND EXISTS (
                    SELECT 1 FROM channel_key ck
                    WHERE ck.channel_id = er.channel_id
                      AND ck.enabled = TRUE
                )
            )
            OR (
                er.use_credentials = TRUE
                AND EXISTS (
                    SELECT 1 FROM credential cr
                    JOIN channel c ON c.id = er.channel_id
                    WHERE cr.provider = c.provider
                      AND cr.enabled = TRUE
                )
            )
        )
        SELECT
            (SELECT count(*) FROM base) AS total_models,
            (SELECT count(*) FROM eligible_model) AS eligible_models,
            (SELECT count(*) FROM endpoint_ready) AS endpoint_ready_models,
            (SELECT count(*) FROM secret_ready) AS callable_models
        "#,
    )
    .fetch_one(&state.db.pool)
    .await?;
    let total_models: i64 = row.try_get("total_models")?;
    let eligible_models: i64 = row.try_get("eligible_models")?;
    let endpoint_ready_models: i64 = row.try_get("endpoint_ready_models")?;
    let callable_models: i64 = row.try_get("callable_models")?;
    let blocked_at = if total_models == 0 {
        "no_channel_models"
    } else if eligible_models == 0 {
        "model_or_channel_disabled"
    } else if endpoint_ready_models == 0 {
        "no_enabled_matching_endpoint"
    } else {
        "no_enabled_key_or_credential"
    };
    tracing::info!(
        total_models,
        eligible_models,
        endpoint_ready_models,
        callable_models,
        blocked_at,
        "no callable admin default text model candidate found"
    );
    Ok(())
}

async fn admin_model_record_from_stored(
    state: &AppState,
    setting: StoredAdminModelSetting,
    updated_at: Option<DateTime<Utc>>,
) -> AppResult<AdminModelSettingRecord> {
    let channel_name = if let Some(channel_id) = setting.default_text_channel_id {
        sqlx::query_scalar::<_, String>("SELECT name FROM channel WHERE id = $1")
            .bind(channel_id)
            .fetch_optional(&state.db.pool)
            .await?
    } else {
        None
    };
    Ok(AdminModelSettingRecord {
        default_text_model: setting.default_text_model,
        default_text_channel_id: setting.default_text_channel_id,
        default_text_channel_name: channel_name,
        updated_at,
    })
}

pub async fn get_smtp_setting(state: &AppState) -> AppResult<SmtpSettingRecord> {
    let Some(row) = sqlx::query("SELECT value, updated_at FROM setting WHERE key = $1")
        .bind(SMTP_SETTING_KEY)
        .fetch_optional(&state.db.pool)
        .await?
    else {
        return Ok(empty_smtp_setting());
    };

    let value: serde_json::Value = row.try_get("value")?;
    let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
    let setting: StoredSmtpSetting = serde_json::from_value(value)?;
    Ok(record_from_stored(setting, Some(updated_at)))
}

/// 校验并加密 SMTP 设置，返回待写入的 JSON 值。不触碰数据库写入，
/// 供普通路径与 setup 事务路径共用，避免逻辑重复。
async fn prepare_smtp_setting_value(
    state: &AppState,
    req: UpsertSmtpSettingRequest,
) -> AppResult<serde_json::Value> {
    let existing = existing_smtp_setting(state).await?;
    let smtp_host = required_trimmed(req.smtp_host, "SMTP host is required")?;
    let from_email = required_trimmed(req.from_email, "sender email is required")?;
    validate_smtp_port(req.smtp_port)?;
    validate_from_email(&from_email)?;

    let next_password = if req.clear_smtp_password {
        None
    } else if let Some(password) = optional_trimmed(req.smtp_password) {
        Some(state.secrets.encrypt(&password)?)
    } else {
        existing.and_then(|setting| setting.password_ciphertext)
    };

    let setting = StoredSmtpSetting {
        smtp_host,
        smtp_port: req.smtp_port,
        smtp_username: optional_trimmed(req.smtp_username),
        password_ciphertext: next_password,
        smtp_tls: req.smtp_tls,
        from_email,
        from_name: optional_trimmed(req.from_name),
        subject_prefix: optional_trimmed(req.subject_prefix),
    };
    Ok(serde_json::to_value(&setting)?)
}

pub async fn upsert_smtp_setting(
    state: &AppState,
    req: UpsertSmtpSettingRequest,
) -> AppResult<SmtpSettingRecord> {
    let value = prepare_smtp_setting_value(state, req).await?;
    let row = sqlx::query(
        r#"
        INSERT INTO setting (key, value)
        VALUES ($1, $2)
        ON CONFLICT (key)
        DO UPDATE SET value = EXCLUDED.value, updated_at = now()
        RETURNING value, updated_at
        "#,
    )
    .bind(SMTP_SETTING_KEY)
    .bind(value)
    .fetch_one(&state.db.pool)
    .await?;

    let value: serde_json::Value = row.try_get("value")?;
    let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
    let setting: StoredSmtpSetting = serde_json::from_value(value)?;
    Ok(record_from_stored(setting, Some(updated_at)))
}

/// setup 事务专用：在给定事务内写入 SMTP 设置，与 service_policy 同一事务提交，
/// 避免 setup 失败时留下孤立的 SMTP 配置。
pub(crate) async fn upsert_smtp_setting_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    state: &AppState,
    req: UpsertSmtpSettingRequest,
) -> AppResult<()> {
    let value = prepare_smtp_setting_value(state, req).await?;
    sqlx::query(
        r#"
        INSERT INTO setting (key, value)
        VALUES ($1, $2)
        ON CONFLICT (key)
        DO UPDATE SET value = EXCLUDED.value, updated_at = now()
        "#,
    )
    .bind(SMTP_SETTING_KEY)
    .bind(value)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn test_smtp_setting(
    state: &AppState,
    req: UpsertSmtpSettingRequest,
) -> AppResult<TestSmtpSettingResponse> {
    let config = runtime_config_from_request(state, req).await?;
    EmailService::send_test(&config, &config.from_email)
        .await
        .map_err(|err| {
            tracing::warn!(error = ?err, "failed to send SMTP test email");
            let (code, message) = smtp_test_error(&err);
            AppError::BadRequestWithCode { code, message }
        })?;
    Ok(TestSmtpSettingResponse { ok: true })
}

async fn existing_smtp_setting(state: &AppState) -> AppResult<Option<StoredSmtpSetting>> {
    let Some(row) = sqlx::query("SELECT value FROM setting WHERE key = $1")
        .bind(SMTP_SETTING_KEY)
        .fetch_optional(&state.db.pool)
        .await?
    else {
        return Ok(None);
    };
    let value: serde_json::Value = row.try_get("value")?;
    Ok(Some(serde_json::from_value(value)?))
}

async fn runtime_config_from_request(
    state: &AppState,
    req: UpsertSmtpSettingRequest,
) -> AppResult<EmailConfig> {
    let existing = existing_smtp_setting(state).await?;
    let smtp_host = required_trimmed(req.smtp_host, "SMTP host is required")?;
    let from_email = required_trimmed(req.from_email, "sender email is required")?;
    validate_smtp_port(req.smtp_port)?;
    validate_from_email(&from_email)?;

    let smtp_password = if req.clear_smtp_password {
        None
    } else if let Some(password) = optional_trimmed(req.smtp_password) {
        Some(password)
    } else {
        existing
            .and_then(|setting| setting.password_ciphertext)
            .map(|ciphertext| state.secrets.plaintext(0, &ciphertext))
            .transpose()?
    };

    Ok(EmailConfig {
        smtp_host,
        smtp_port: req.smtp_port,
        smtp_username: optional_trimmed(req.smtp_username),
        smtp_password,
        smtp_tls: req.smtp_tls,
        from_email,
        from_name: optional_trimmed(req.from_name),
        subject_prefix: optional_trimmed(req.subject_prefix),
    })
}

fn empty_smtp_setting() -> SmtpSettingRecord {
    SmtpSettingRecord {
        configured: false,
        smtp_host: String::new(),
        smtp_port: 587,
        smtp_username: None,
        smtp_password_set: false,
        smtp_tls: true,
        from_email: String::new(),
        from_name: None,
        subject_prefix: None,
        updated_at: None,
    }
}

fn record_from_stored(
    setting: StoredSmtpSetting,
    updated_at: Option<DateTime<Utc>>,
) -> SmtpSettingRecord {
    SmtpSettingRecord {
        configured: true,
        smtp_host: setting.smtp_host,
        smtp_port: setting.smtp_port,
        smtp_username: setting.smtp_username,
        smtp_password_set: setting.password_ciphertext.is_some(),
        smtp_tls: setting.smtp_tls,
        from_email: setting.from_email,
        from_name: setting.from_name,
        subject_prefix: setting.subject_prefix,
        updated_at,
    }
}

fn required_trimmed(value: String, message: &str) -> AppResult<String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(AppError::BadRequest(message.to_string()));
    }
    Ok(value)
}

fn optional_trimmed(value: Option<String>) -> Option<String> {
    trimmed_non_empty_owned(value.as_deref())
}

fn validate_smtp_port(port: u16) -> AppResult<()> {
    if port == 0 {
        return Err(AppError::BadRequest("SMTP port is invalid".to_string()));
    }
    Ok(())
}

fn validate_from_email(email: &str) -> AppResult<()> {
    email
        .parse::<Mailbox>()
        .map(|_| ())
        .map_err(|_| AppError::BadRequest("sender email is invalid".to_string()))
}
