use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, TimeZone, Utc};
use hmac::{Hmac, KeyInit, Mac};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;

use crate::AppState;

use super::credentials::{CredentialQuota, ParsedCredential, QuotaWindow};

pub(super) const OPENAI_PROVIDER: &str = "openai";
const OPENAI_USAGE_URL: &str = "https://chatgpt.com/backend-api/codex/usage";
const OPENAI_OAUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_OAUTH_CLIENT_ID: &str = "codex-cli";

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Deserialize)]
struct OpenAiCredential {
    auth_mode: Option<String>,
    #[serde(rename = "OPENAI_API_KEY")]
    openai_api_key: Option<String>,
    tokens: Option<OpenAiTokens>,
    last_refresh: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiTokens {
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RefreshTokenResponse {
    pub(super) access_token: Option<String>,
    pub(super) refresh_token: Option<String>,
    pub(super) id_token: Option<String>,
    pub(super) expires_in: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct OpenAiRuntimeCredential {
    pub access_token: String,
    pub account_id: Option<String>,
}

pub(super) fn detect_openai_credential(
    state: &AppState,
    value: &Value,
) -> Option<ParsedCredential> {
    let parsed: OpenAiCredential = serde_json::from_value(value.clone()).ok()?;
    let has_api_key = parsed
        .openai_api_key
        .as_deref()
        .map(str::trim)
        .is_some_and(|secret| !secret.is_empty());
    let tokens = parsed.tokens.as_ref();
    let has_access_token = tokens
        .and_then(|tokens| tokens.access_token.as_deref())
        .map(str::trim)
        .is_some_and(|token| !token.is_empty());
    let has_refresh_token = tokens
        .and_then(|tokens| tokens.refresh_token.as_deref())
        .map(str::trim)
        .is_some_and(|token| !token.is_empty());
    let has_id_token = tokens
        .and_then(|tokens| tokens.id_token.as_deref())
        .map(str::trim)
        .is_some_and(|token| !token.is_empty());
    if !has_api_key && !has_access_token && !has_refresh_token && !has_id_token {
        return None;
    }

    let id_claims = tokens
        .and_then(|tokens| tokens.id_token.as_deref())
        .and_then(decode_jwt_claims);
    let access_claims = tokens
        .and_then(|tokens| tokens.access_token.as_deref())
        .and_then(decode_jwt_claims);
    let email = id_claims
        .as_ref()
        .and_then(|claims| string_field(claims, &["email"]))
        .or_else(|| {
            access_claims.as_ref().and_then(|claims| {
                nested_string_field(claims, "https://api.openai.com/profile", &["email"])
            })
        });
    let account_id = tokens
        .and_then(|tokens| tokens.account_id.clone())
        .or_else(|| openai_account_id_from_claims(id_claims.as_ref()))
        .or_else(|| openai_account_id_from_claims(access_claims.as_ref()));
    let plan_type = openai_plan_type_from_claims(id_claims.as_ref())
        .or_else(|| openai_plan_type_from_claims(access_claims.as_ref()));
    let (identity_kind, identity_value) = if let Some(account_id) = account_id.as_deref() {
        ("chatgpt", account_id.to_string())
    } else if let Some(api_key) = parsed
        .openai_api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        ("api_key", api_key.to_string())
    } else {
        return None;
    };
    let identity_hash = identity_hash(
        &state.config.upstream_secret_key,
        OPENAI_PROVIDER,
        identity_kind,
        &identity_value,
    );
    let identity_label = email
        .clone()
        .or_else(|| account_id.as_deref().map(mask_middle))
        .or_else(|| parsed.openai_api_key.as_deref().and_then(mask_secret));

    Some(ParsedCredential {
        provider: OPENAI_PROVIDER.to_string(),
        identity_hash,
        identity_label,
        auth_mode: parsed.auth_mode,
        api_key_preview: parsed.openai_api_key.as_deref().and_then(mask_secret),
        has_oauth_tokens: has_access_token || has_refresh_token || has_id_token,
        has_refresh_token,
        has_id_token,
        email,
        account_id,
        plan_type: plan_type.clone(),
        last_refresh: parsed.last_refresh,
        metadata: json!({
            "identity_kind": identity_kind,
            "plan_type": plan_type
        }),
    })
}

fn identity_hash(secret: &str, provider: &str, kind: &str, value: &str) -> String {
    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(secret.as_bytes())
        .expect("hmac accepts any key length");
    mac.update(provider.as_bytes());
    mac.update(b":");
    mac.update(kind.as_bytes());
    mac.update(b":");
    mac.update(value.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub(super) async fn refresh_openai_quota(
    state: &AppState,
    id: crate::id::DbId,
    value: &mut Value,
) -> CredentialQuota {
    let Some(mut access_token) = credential_access_token(value) else {
        return unavailable_quota("OAuth access token is missing");
    };

    match query_openai_quota(state, &access_token).await {
        Ok(quota) => quota,
        Err(QueryQuotaError::Unauthorized) => {
            let Some(refresh_token) = credential_refresh_token(value) else {
                return failed_quota("access token expired and refresh token is missing");
            };
            match refresh_openai_token(state, &refresh_token).await {
                Ok(tokens) => {
                    if let Some(token) = tokens.access_token {
                        access_token = token;
                        update_token_value(value, "access_token", access_token.clone());
                    }
                    if let Some(token) = tokens.refresh_token {
                        update_token_value(value, "refresh_token", token);
                    }
                    if let Some(token) = tokens.id_token {
                        update_token_value(value, "id_token", token);
                    }
                    if let Some(expires_in) = tokens.expires_in {
                        let expires_at = Utc::now() + Duration::seconds(expires_in);
                        update_token_value(value, "expires_at", expires_at.timestamp().to_string());
                    }
                    value["last_refresh"] = Value::String(Utc::now().to_rfc3339());
                    state.secrets.forget(id);
                    match query_openai_quota(state, &access_token).await {
                        Ok(quota) => quota,
                        Err(err) => err.into_quota(),
                    }
                }
                Err(err) => failed_quota(format!("token refresh failed: {err}")),
            }
        }
        Err(err) => err.into_quota(),
    }
}

async fn query_openai_quota(
    state: &AppState,
    access_token: &str,
) -> Result<CredentialQuota, QueryQuotaError> {
    let response = state
        .http
        .get(OPENAI_USAGE_URL)
        .bearer_auth(access_token)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|err| QueryQuotaError::Request(err.to_string()))?;
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(QueryQuotaError::Unauthorized);
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(QueryQuotaError::Request(
            "quota endpoint is rate limited".to_string(),
        ));
    }
    if !status.is_success() {
        return Err(QueryQuotaError::Unavailable(format!(
            "quota endpoint returned {}",
            status.as_u16()
        )));
    }
    let value = response
        .json::<Value>()
        .await
        .map_err(|err| QueryQuotaError::Request(err.to_string()))?;
    Ok(quota_from_value(&value))
}

pub(super) async fn refresh_openai_token(
    state: &AppState,
    refresh_token: &str,
) -> Result<RefreshTokenResponse, String> {
    let response = state
        .http
        .post(OPENAI_OAUTH_TOKEN_URL)
        .json(&json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": OPENAI_OAUTH_CLIENT_ID
        }))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("token endpoint returned {}", status.as_u16()));
    }
    response
        .json::<RefreshTokenResponse>()
        .await
        .map_err(|err| err.to_string())
}

pub fn openai_runtime_secret(value: &Value) -> Option<String> {
    openai_runtime_credential(value).map(|credential| credential.access_token)
}

pub fn openai_runtime_credential(value: &Value) -> Option<OpenAiRuntimeCredential> {
    if let Some(api_key) = value
        .get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|secret| !secret.is_empty())
        .map(str::to_string)
    {
        return Some(OpenAiRuntimeCredential {
            access_token: api_key,
            account_id: None,
        });
    }

    let access_token = credential_access_token(value)?;
    Some(OpenAiRuntimeCredential {
        account_id: openai_account_id(value),
        access_token,
    })
}

pub(super) fn openai_account_id(value: &Value) -> Option<String> {
    value
        .get("tokens")
        .and_then(|tokens| tokens.get("account_id"))
        .or_else(|| value.get("account_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|account_id| !account_id.is_empty())
        .map(str::to_string)
        .or_else(|| {
            value
                .get("tokens")
                .and_then(|tokens| tokens.get("id_token"))
                .or_else(|| value.get("id_token"))
                .and_then(Value::as_str)
                .and_then(decode_jwt_claims)
                .as_ref()
                .and_then(|claims| openai_account_id_from_claims(Some(claims)))
        })
        .or_else(|| {
            credential_access_token(value)
                .and_then(|token| decode_jwt_claims(&token))
                .as_ref()
                .and_then(|claims| openai_account_id_from_claims(Some(claims)))
        })
}

fn decode_jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn openai_account_id_from_claims(claims: Option<&Value>) -> Option<String> {
    let claims = claims?;
    nested_string_field(
        claims,
        "https://api.openai.com/auth",
        &["chatgpt_account_id", "account_id"],
    )
    .or_else(|| string_field(claims, &["account_id", "sub"]))
}

fn openai_plan_type_from_claims(claims: Option<&Value>) -> Option<String> {
    let claims = claims?;
    nested_string_field(
        claims,
        "https://api.openai.com/auth",
        &["chatgpt_plan_type", "plan_type"],
    )
    .or_else(|| string_field(claims, &["chatgpt_plan_type", "plan_type"]))
    .map(|plan| plan.to_ascii_lowercase())
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(text) = value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return Some(text.to_string());
        }
    }
    None
}

fn nested_string_field(value: &Value, object_key: &str, keys: &[&str]) -> Option<String> {
    let object = value.get(object_key)?;
    string_field(object, keys)
}

fn mask_secret(secret: &str) -> Option<String> {
    let secret = secret.trim();
    if secret.is_empty() {
        return None;
    }
    if secret.len() <= 12 {
        return Some("****".to_string());
    }
    let start = &secret[..secret.len().min(6)];
    let end = &secret[secret.len().saturating_sub(4)..];
    Some(format!("{start}...{end}"))
}

fn mask_middle(value: &str) -> String {
    let value = value.trim();
    if value.len() <= 12 {
        return value.to_string();
    }
    format!(
        "{}...{}",
        &value[..6],
        &value[value.len().saturating_sub(4)..]
    )
}

pub(super) fn credential_access_token(value: &Value) -> Option<String> {
    value
        .get("tokens")
        .and_then(|tokens| tokens.get("access_token"))
        .or_else(|| value.get("access_token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

pub(super) fn credential_refresh_token(value: &Value) -> Option<String> {
    value
        .get("tokens")
        .and_then(|tokens| tokens.get("refresh_token"))
        .or_else(|| value.get("refresh_token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

pub(super) fn update_token_value(value: &mut Value, key: &str, token: String) {
    if value.get("tokens").is_some() {
        if !value["tokens"].is_object() {
            value["tokens"] = json!({});
        }
        value["tokens"][key] = Value::String(token);
    } else {
        value[key] = Value::String(token);
    }
}

fn quota_from_value(value: &Value) -> CredentialQuota {
    CredentialQuota {
        status: "ok".to_string(),
        message: string_field(value, &["message", "detail"]),
        plan: string_field(value, &["plan_type", "plan", "limit_name"]),
        five_hour: find_quota_window(
            value,
            &[
                "five_hour",
                "fiveHour",
                "5h",
                "primary",
                "primary_window",
                "short",
            ],
        ),
        weekly: find_quota_window(
            value,
            &[
                "weekly",
                "week",
                "7d",
                "secondary",
                "secondary_window",
                "long",
            ],
        ),
        updated_at: Utc::now(),
    }
}

fn find_quota_window(value: &Value, names: &[&str]) -> Option<QuotaWindow> {
    for name in names {
        if let Some(window) = value.get(*name).and_then(quota_window_from_value) {
            return Some(window);
        }
    }
    if let Some(object) = value.as_object() {
        for nested in object.values() {
            if let Some(window) = find_quota_window(nested, names) {
                return Some(window);
            }
        }
    }
    None
}

fn quota_window_from_value(value: &Value) -> Option<QuotaWindow> {
    let percent = number_field(
        value,
        &[
            "remaining_percent",
            "percent",
            "percentage",
            "available_percent",
        ],
    )
    .or_else(|| {
        number_field(
            value,
            &["used_percent", "usage_percent", "utilization_percent"],
        )
        .map(|used_percent| 100.0 - used_percent)
    })
    .or_else(|| {
        let used = number_field(value, &["used", "usage", "consumed"])?;
        let limit = number_field(value, &["limit", "total", "quota"])?;
        (limit > 0.0).then_some(((limit - used).max(0.0) / limit) * 100.0)
    })
    .map(|percent| percent.clamp(0.0, 100.0));
    let used = number_field(value, &["used", "usage", "consumed"]);
    let limit = number_field(value, &["limit", "total", "quota"]);
    if percent.is_none() && used.is_none() && limit.is_none() {
        return None;
    }
    Some(QuotaWindow {
        percent,
        used,
        limit,
        reset_at: datetime_field(value, &["reset_at", "resets_at", "reset"]),
    })
}

fn number_field(value: &Value, keys: &[&str]) -> Option<f64> {
    for key in keys {
        let current = value.get(*key)?;
        if let Some(number) = current.as_f64() {
            return Some(number);
        }
        if let Some(text) = current.as_str() {
            if let Ok(number) = text.parse::<f64>() {
                return Some(number);
            }
        }
    }
    None
}

fn datetime_field(value: &Value, keys: &[&str]) -> Option<DateTime<Utc>> {
    for key in keys {
        let current = value.get(*key)?;
        if let Some(text) = current.as_str() {
            if let Ok(value) = DateTime::parse_from_rfc3339(text) {
                return Some(value.with_timezone(&Utc));
            }
            if let Ok(timestamp) = text.parse::<i64>() {
                return timestamp_to_datetime(timestamp);
            }
        }
        if let Some(timestamp) = current
            .as_i64()
            .or_else(|| current.as_u64().map(|v| v as i64))
        {
            return timestamp_to_datetime(timestamp);
        }
    }
    None
}

fn timestamp_to_datetime(timestamp: i64) -> Option<DateTime<Utc>> {
    if timestamp > 10_000_000_000 {
        Utc.timestamp_millis_opt(timestamp).single()
    } else {
        Utc.timestamp_opt(timestamp, 0).single()
    }
}

#[derive(Debug)]
enum QueryQuotaError {
    Unauthorized,
    Unavailable(String),
    Request(String),
}

impl QueryQuotaError {
    fn into_quota(self) -> CredentialQuota {
        match self {
            Self::Unauthorized => failed_quota("credential is unauthorized"),
            Self::Unavailable(message) => unavailable_quota(message),
            Self::Request(message) => failed_quota(message),
        }
    }
}

fn failed_quota(message: impl Into<String>) -> CredentialQuota {
    quota_status("failed", message)
}

fn unavailable_quota(message: impl Into<String>) -> CredentialQuota {
    quota_status("unavailable", message)
}

fn quota_status(status: &'static str, message: impl Into<String>) -> CredentialQuota {
    CredentialQuota {
        status: status.to_string(),
        message: Some(message.into()),
        plan: None,
        five_hour: None,
        weekly: None,
        updated_at: Utc::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_claims_parse_openai_account_id() {
        let claims = URL_SAFE_NO_PAD.encode(
            br#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acct_1"},"email":"user@example.com"}"#,
        );
        let token = format!("header.{claims}.sig");
        let decoded = decode_jwt_claims(&token).unwrap();
        assert_eq!(
            openai_account_id_from_claims(Some(&decoded)).as_deref(),
            Some("acct_1")
        );
    }

    #[test]
    fn jwt_claims_parse_openai_plan_type() {
        let claims = URL_SAFE_NO_PAD
            .encode(br#"{"https://api.openai.com/auth":{"chatgpt_plan_type":"Plus"}}"#);
        let token = format!("header.{claims}.sig");
        let decoded = decode_jwt_claims(&token).unwrap();
        assert_eq!(
            openai_plan_type_from_claims(Some(&decoded)).as_deref(),
            Some("plus")
        );
    }

    #[test]
    fn hmac_identity_is_stable_without_exposing_value() {
        let left = identity_hash("secret", "openai", "chatgpt", "acct_1");
        let right = identity_hash("secret", "openai", "chatgpt", "acct_1");
        assert_eq!(left, right);
        assert!(!left.contains("acct_1"));
    }

    #[test]
    fn quota_window_calculates_remaining_percent() {
        let value = json!({
            "five_hour": {
                "used": 25,
                "limit": 100,
                "reset_at": "2026-05-30T00:00:00Z"
            }
        });
        let quota = quota_from_value(&value);
        let window = quota.five_hour.unwrap();
        assert_eq!(window.percent, Some(75.0));
        assert!(window.reset_at.is_some());
    }
}
