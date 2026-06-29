use chrono::{DateTime, Utc};
use lettre::message::Mailbox;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    config::RuntimeProbe,
    email::{smtp_test_error, EmailConfig, EmailService, SMTP_SETTING_KEY},
    error::{AppError, AppResult},
    input::trimmed_non_empty_owned,
    setup::bootstrap::{save_public_base_url_config, validate_public_base_url},
    AppState,
};

const SITE_BRAND_SETTING_KEY: &str = "site_brand";
const DEFAULT_LOGO_URL: &str = "/logos/logo.svg";

#[derive(Debug, Serialize)]
pub struct SiteSettingRecord {
    pub site_name: String,
    pub public_base_url: Option<String>,
    pub logo_url: Option<String>,
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

pub async fn get_site_setting(state: &AppState) -> AppResult<SiteSettingRecord> {
    let probe = RuntimeProbe::from_env()?;
    let env_write_supported = !probe.runtime_mode.is_distributed();
    let brand = existing_site_brand_setting(state).await?;
    let site_name = brand
        .as_ref()
        .and_then(|setting| setting.site_name.clone())
        .or(probe.site_name)
        .unwrap_or_else(|| "NeoGate".to_string());
    let logo_url = brand.map_or_else(
        || Some(DEFAULT_LOGO_URL.to_string()),
        |setting| setting.logo_url,
    );
    Ok(SiteSettingRecord {
        site_name,
        public_base_url: probe.public_base_url,
        logo_url,
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

pub async fn upsert_smtp_setting(
    state: &AppState,
    req: UpsertSmtpSettingRequest,
) -> AppResult<SmtpSettingRecord> {
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
    let value = serde_json::to_value(&setting)?;
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
