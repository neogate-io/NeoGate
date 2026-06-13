use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    config::{PaymentConfig, PaymentProvider, ZpayConfig},
    error::{AppError, AppResult},
    AppState,
};

pub const PAYMENT_SETTING_KEY: &str = "payment";

#[derive(Debug, Serialize)]
pub struct PaymentSettingRecord {
    pub configured: bool,
    pub payment_enabled: bool,
    pub return_base_url: Option<String>,
    pub zpay_api_url: String,
    pub zpay_merchant_id: Option<String>,
    pub zpay_secret_key_set: bool,
    pub zpay_default_pay_type: String,
    pub zpay_site_name: String,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertPaymentSettingRequest {
    pub payment_enabled: bool,
    pub return_base_url: Option<String>,
    pub zpay_api_url: String,
    pub zpay_merchant_id: Option<String>,
    pub zpay_secret_key: Option<String>,
    #[serde(default)]
    pub clear_zpay_secret_key: bool,
    pub zpay_default_pay_type: String,
    pub zpay_site_name: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredPaymentSetting {
    payment_enabled: bool,
    return_base_url: Option<String>,
    zpay_api_url: Option<String>,
    zpay_merchant_id: Option<String>,
    zpay_secret_key_ciphertext: Option<String>,
    zpay_default_pay_type: String,
    zpay_site_name: String,
}

pub async fn get_payment_setting(state: &AppState) -> AppResult<PaymentSettingRecord> {
    let Some(row) = sqlx::query("SELECT value, updated_at FROM setting WHERE key = $1")
        .bind(PAYMENT_SETTING_KEY)
        .fetch_optional(&state.db.pool)
        .await?
    else {
        let config = PaymentConfig::default_for_site(&state.config.site_name);
        return Ok(record_from_config(&config, false, None));
    };

    let value: serde_json::Value = row.try_get("value")?;
    let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
    let setting: StoredPaymentSetting = serde_json::from_value(value)?;
    Ok(record_from_stored(setting, true, Some(updated_at)))
}

pub async fn upsert_payment_setting(
    state: &AppState,
    req: UpsertPaymentSettingRequest,
) -> AppResult<PaymentSettingRecord> {
    let existing = existing_payment_setting(state).await?;
    let return_base_url = optional_trimmed(req.return_base_url);
    let zpay_api_url = optional_trimmed(Some(req.zpay_api_url))
        .unwrap_or_else(|| default_zpay_api_url().to_string());
    let zpay_merchant_id = optional_trimmed(req.zpay_merchant_id);
    let zpay_default_pay_type =
        optional_trimmed(Some(req.zpay_default_pay_type)).unwrap_or_else(|| "wxpay".to_string());
    let zpay_site_name =
        optional_trimmed(Some(req.zpay_site_name)).unwrap_or_else(|| "NeoGate".to_string());

    let zpay_secret_key_ciphertext = if req.clear_zpay_secret_key {
        None
    } else if let Some(secret_key) = optional_trimmed(req.zpay_secret_key) {
        Some(state.secrets.encrypt(&secret_key)?)
    } else if let Some(setting) = existing {
        setting.zpay_secret_key_ciphertext
    } else {
        None
    };

    if req.payment_enabled {
        validate_enabled_payment(
            return_base_url.as_deref(),
            Some(zpay_api_url.as_str()),
            zpay_merchant_id.as_deref(),
            zpay_secret_key_ciphertext.as_deref(),
        )?;
    }

    let setting = StoredPaymentSetting {
        payment_enabled: req.payment_enabled,
        return_base_url,
        zpay_api_url: Some(zpay_api_url),
        zpay_merchant_id,
        zpay_secret_key_ciphertext,
        zpay_default_pay_type,
        zpay_site_name,
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
    .bind(PAYMENT_SETTING_KEY)
    .bind(value)
    .fetch_one(&state.db.pool)
    .await?;

    let value: serde_json::Value = row.try_get("value")?;
    let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
    let setting: StoredPaymentSetting = serde_json::from_value(value)?;
    Ok(record_from_stored(setting, true, Some(updated_at)))
}

pub async fn runtime_payment_config(state: &AppState) -> AppResult<PaymentConfig> {
    let Some(setting) = existing_payment_setting(state).await? else {
        return Ok(PaymentConfig::default_for_site(&state.config.site_name));
    };

    let secret_key = setting
        .zpay_secret_key_ciphertext
        .as_deref()
        .map(|ciphertext| state.secrets.plaintext(0, ciphertext))
        .transpose()?;

    Ok(PaymentConfig {
        enabled_providers: if setting.payment_enabled {
            vec![PaymentProvider::Zpay]
        } else {
            Vec::new()
        },
        return_base_url: setting.return_base_url,
        zpay: ZpayConfig {
            api_url: setting.zpay_api_url,
            merchant_id: setting.zpay_merchant_id,
            secret_key,
            default_pay_type: setting.zpay_default_pay_type,
            site_name: setting.zpay_site_name,
        },
    })
}

async fn existing_payment_setting(state: &AppState) -> AppResult<Option<StoredPaymentSetting>> {
    let Some(row) = sqlx::query("SELECT value FROM setting WHERE key = $1")
        .bind(PAYMENT_SETTING_KEY)
        .fetch_optional(&state.db.pool)
        .await?
    else {
        return Ok(None);
    };
    let value: serde_json::Value = row.try_get("value")?;
    Ok(Some(serde_json::from_value(value)?))
}

fn record_from_config(
    config: &PaymentConfig,
    configured: bool,
    updated_at: Option<DateTime<Utc>>,
) -> PaymentSettingRecord {
    PaymentSettingRecord {
        configured,
        payment_enabled: config.provider_enabled(PaymentProvider::Zpay),
        return_base_url: config.return_base_url.clone(),
        zpay_api_url: config
            .zpay
            .api_url
            .clone()
            .unwrap_or_else(|| default_zpay_api_url().to_string()),
        zpay_merchant_id: config.zpay.merchant_id.clone(),
        zpay_secret_key_set: config.zpay.secret_key.is_some(),
        zpay_default_pay_type: config.zpay.default_pay_type.clone(),
        zpay_site_name: config.zpay.site_name.clone(),
        updated_at,
    }
}

fn record_from_stored(
    setting: StoredPaymentSetting,
    configured: bool,
    updated_at: Option<DateTime<Utc>>,
) -> PaymentSettingRecord {
    PaymentSettingRecord {
        configured,
        payment_enabled: setting.payment_enabled,
        return_base_url: setting.return_base_url,
        zpay_api_url: setting
            .zpay_api_url
            .unwrap_or_else(|| default_zpay_api_url().to_string()),
        zpay_merchant_id: setting.zpay_merchant_id,
        zpay_secret_key_set: setting.zpay_secret_key_ciphertext.is_some(),
        zpay_default_pay_type: setting.zpay_default_pay_type,
        zpay_site_name: setting.zpay_site_name,
        updated_at,
    }
}

fn validate_enabled_payment(
    return_base_url: Option<&str>,
    zpay_api_url: Option<&str>,
    zpay_merchant_id: Option<&str>,
    zpay_secret_key_ciphertext: Option<&str>,
) -> AppResult<()> {
    if return_base_url.is_none() {
        return Err(AppError::BadRequest(
            "payment return base URL is required".to_string(),
        ));
    }
    if zpay_api_url.is_none() {
        return Err(AppError::BadRequest("ZPAY API URL is required".to_string()));
    }
    if zpay_merchant_id.is_none() {
        return Err(AppError::BadRequest(
            "ZPAY merchant ID is required".to_string(),
        ));
    }
    if zpay_secret_key_ciphertext.is_none() {
        return Err(AppError::BadRequest(
            "ZPAY secret key is required".to_string(),
        ));
    }
    Ok(())
}

fn optional_trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn default_zpay_api_url() -> &'static str {
    "https://zpayz.cn/submit.php"
}
