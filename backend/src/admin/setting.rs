use chrono::{DateTime, Utc};
use lettre::message::Mailbox;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    AppState,
    email::{EmailConfig, EmailService, SMTP_SETTING_KEY, smtp_test_error},
    error::{AppError, AppResult},
};

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
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
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
