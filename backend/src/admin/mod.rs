pub(crate) mod channel;
pub(crate) mod credentials;
mod openai;
pub(crate) mod price;
pub(crate) mod provider;
pub(crate) mod setting;
mod user;

use std::sync::Arc;

use axum::{
    extract::{Multipart, Path, Query, State},
    routing::{get, patch, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;

use crate::{
    auth::{self, AdminAuth},
    billing::CreditAccountType,
    cache::InvalidationEvent,
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

use self::{
    channel::{
        create_channel, create_channel_key, delete_channel, delete_channel_key,
        list_all_channel_keys, list_channel_keys, list_channels, update_channel,
        update_channel_key, ChannelKeyRecord, ChannelRecord, CreateChannelKeyRequest,
        CreateChannelRequest, UpdateChannelKeyRequest, UpdateChannelRequest,
    },
    credentials::{
        delete_credential, disable_credential, enable_credential, list_credential_models,
        list_credentials, refresh_credential, reset_credential_model,
        runtime_secret_from_enabled_credential, upload_credentials, CredentialModelRecord,
        CredentialRecord, CredentialUploadResult,
    },
    price::{
        list_pricing_policies, list_pricing_templates, list_provider_models, list_provider_prices,
        sync_pricing_templates, upsert_pricing_policy, upsert_provider_price, PricingPolicyRecord,
        PricingTemplateRecord, PricingTemplateSyncResult, ProviderModelRecord, ProviderPriceRecord,
        SyncPricingTemplatesRequest, UpsertPricingPolicyRequest, UpsertProviderPriceRequest,
    },
    provider::{
        ensure_custom_provider, list_providers, provider_default_endpoints, record_provider_models,
        ProviderRecord, CUSTOM_PROVIDER_CODE, OPENAI_OAUTH_PROTOCOL,
    },
    setting::{
        get_smtp_setting, test_smtp_setting, upsert_smtp_setting, SmtpSettingRecord,
        TestSmtpSettingResponse, UpsertSmtpSettingRequest,
    },
    user::{
        adjust_credit, adjust_user_key_model_credit, create_user, create_user_key, delete_user,
        delete_user_key, list_user_groups, list_user_keys, list_users, update_user,
        update_user_key, CreateUserKeyRequest, CreateUserRequest, CreatedUserKey,
        ListUserKeysQuery, ListUsersQuery, UpdateUserKeyRequest, UpdateUserRequest,
        UserGroupRecord, UserKeyModelCreditRecord, UserKeyPage, UserKeyRecord, UserPage,
        UserRecord,
    },
};
use crate::payment::settings::{
    get_payment_setting, upsert_payment_setting, PaymentSettingRecord, UpsertPaymentSettingRequest,
};

pub use user::public_router;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/admin/users", get(users).post(create_user_handler))
        .route("/api/admin/user-groups", get(user_groups))
        .route(
            "/api/admin/users/{id}",
            patch(update_user_handler).delete(delete_user_handler),
        )
        .route(
            "/api/admin/user-keys",
            get(user_keys).post(create_user_key_handler),
        )
        .route(
            "/api/admin/user-keys/{id}",
            patch(update_user_key_handler).delete(delete_user_key_handler),
        )
        .route(
            "/api/admin/user-key-model-credits",
            post(adjust_user_key_model_credit_handler),
        )
        .route("/api/admin/credits", post(adjust_credit_handler))
        .route(
            "/api/admin/channels",
            get(channels).post(create_channel_handler),
        )
        .route("/api/admin/providers", get(providers))
        .route("/api/admin/provider-models", get(provider_models))
        .route("/api/admin/upstream-models", post(upstream_models))
        .route("/api/admin/pricing-templates", get(pricing_templates))
        .route(
            "/api/admin/pricing-templates/sync",
            post(sync_pricing_templates_handler),
        )
        .route(
            "/api/admin/pricing-policies",
            get(pricing_policies).post(upsert_pricing_policy_handler),
        )
        .route(
            "/api/admin/settings/smtp",
            get(smtp_setting).post(upsert_smtp_setting_handler),
        )
        .route(
            "/api/admin/settings/smtp/test",
            post(test_smtp_setting_handler),
        )
        .route(
            "/api/admin/settings/payment",
            get(payment_setting).post(upsert_payment_setting_handler),
        )
        .route(
            "/api/admin/settings/admin-password",
            post(update_admin_password_handler),
        )
        .route(
            "/api/admin/provider-prices",
            get(provider_prices).post(upsert_provider_price_handler),
        )
        .route("/api/admin/channel-keys", get(all_channel_keys))
        .route("/api/admin/credentials", get(credentials))
        .route(
            "/api/admin/credentials/upload",
            post(upload_credentials_handler),
        )
        .route(
            "/api/admin/credentials/{id}/refresh",
            post(refresh_credential_handler),
        )
        .route(
            "/api/admin/credentials/{id}/models",
            get(credential_models_handler),
        )
        .route(
            "/api/admin/credentials/{id}/models/{model}/reset",
            post(reset_credential_model_handler),
        )
        .route(
            "/api/admin/credentials/{id}/enable",
            post(enable_credential_handler),
        )
        .route(
            "/api/admin/credentials/{id}/disable",
            post(disable_credential_handler),
        )
        .route(
            "/api/admin/credentials/{id}",
            axum::routing::delete(delete_credential_handler),
        )
        .route(
            "/api/admin/channels/{id}",
            patch(update_channel_handler).delete(delete_channel_handler),
        )
        .route(
            "/api/admin/channels/{id}/keys",
            get(channel_keys).post(create_channel_key_handler),
        )
        .route(
            "/api/admin/channels/{id}/keys/{key_id}",
            patch(update_channel_key_handler).delete(delete_channel_key_handler),
        )
        .route("/api/admin/usage", get(usage))
        .route("/api/admin/health", get(health))
}

async fn users(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListUsersQuery>,
    _admin: AdminAuth,
) -> AppResult<Json<UserPage>> {
    Ok(Json(list_users(&state, query).await?))
}

async fn user_groups(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<Vec<UserGroupRecord>>> {
    Ok(Json(list_user_groups(&state).await?))
}

async fn create_user_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<CreateUserRequest>,
) -> AppResult<Json<UserRecord>> {
    let user = create_user(&state, req).await?;
    Ok(Json(user))
}

async fn update_user_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
    Json(req): Json<UpdateUserRequest>,
) -> AppResult<Json<UserRecord>> {
    let user = update_user(&state, id, req).await?;
    invalidate_cache(&state, InvalidationEvent::User { id }).await;
    Ok(Json(user))
}

async fn delete_user_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<Value>> {
    delete_user(&state, id).await?;
    invalidate_cache(&state, InvalidationEvent::User { id }).await;
    Ok(Json(json!({ "ok": true })))
}

async fn user_keys(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListUserKeysQuery>,
    _admin: AdminAuth,
) -> AppResult<Json<UserKeyPage>> {
    Ok(Json(list_user_keys(&state, query).await?))
}

async fn create_user_key_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<CreateUserKeyRequest>,
) -> AppResult<Json<CreatedUserKey>> {
    let key = create_user_key(&state, req).await?;
    Ok(Json(key))
}

async fn update_user_key_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
    Json(req): Json<UpdateUserKeyRequest>,
) -> AppResult<Json<UserKeyRecord>> {
    let key = update_user_key(&state, id, req).await?;
    invalidate_cache(&state, InvalidationEvent::UserKey { id }).await;
    Ok(Json(key))
}

async fn delete_user_key_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<Value>> {
    delete_user_key(&state, id).await?;
    invalidate_cache(&state, InvalidationEvent::UserKey { id }).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
struct AdjustCreditRequest {
    credit_account_type: String,
    owner_id: DbId,
    amount_micro_usd: i64,
    #[serde(default = "default_credit_reason")]
    reason: String,
}

#[derive(Debug, Deserialize)]
struct AdjustUserKeyModelCreditRequest {
    user_key_id: DbId,
    model: String,
    amount_micro_usd: i64,
    #[serde(default = "default_credit_reason")]
    reason: String,
}

#[derive(Debug, Serialize)]
struct AdjustCreditResponse {
    balance_micro_usd: i64,
}

fn default_credit_reason() -> String {
    "recharge".to_string()
}

async fn adjust_credit_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<AdjustCreditRequest>,
) -> AppResult<Json<AdjustCreditResponse>> {
    let credit_account_type = match req.credit_account_type.as_str() {
        "user" => CreditAccountType::User,
        "user_key" => CreditAccountType::UserKey,
        "user_key_model" => CreditAccountType::UserKeyModel,
        other => {
            return Err(AppError::BadRequest(format!(
                "invalid credit_account type: {other}"
            )))
        }
    };
    let balance_micro_usd = adjust_credit(
        &state,
        credit_account_type,
        req.owner_id,
        req.amount_micro_usd,
        &req.reason,
    )
    .await?;
    Ok(Json(AdjustCreditResponse { balance_micro_usd }))
}

async fn adjust_user_key_model_credit_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<AdjustUserKeyModelCreditRequest>,
) -> AppResult<Json<UserKeyModelCreditRecord>> {
    let record = adjust_user_key_model_credit(
        &state,
        req.user_key_id,
        req.model,
        req.amount_micro_usd,
        &req.reason,
    )
    .await?;
    Ok(Json(record))
}

async fn channels(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<Vec<ChannelRecord>>> {
    Ok(Json(list_channels(&state).await?))
}

async fn providers(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<Vec<ProviderRecord>>> {
    Ok(Json(list_providers(&state).await?))
}

#[derive(Debug, Deserialize)]
struct FetchUpstreamModelsRequest {
    channel_id: Option<DbId>,
    provider: String,
    protocol: Option<String>,
    base_url: Option<String>,
    secret: Option<String>,
    #[serde(default)]
    use_credentials: bool,
}

#[derive(Debug, Serialize)]
struct FetchUpstreamModelsResponse {
    models: Vec<String>,
}

async fn upstream_models(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<FetchUpstreamModelsRequest>,
) -> AppResult<Json<FetchUpstreamModelsResponse>> {
    let provider_code = req.provider.trim();
    let secret = req
        .secret
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if provider_code.is_empty() {
        return Err(AppError::BadRequest("provider is required".to_string()));
    }
    if provider_code == CUSTOM_PROVIDER_CODE {
        ensure_custom_provider(&state).await?;
    }

    let defaults = provider_default_endpoints(&state, provider_code)
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("invalid provider: {provider_code}")))?;
    let protocol = req
        .protocol
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| AppError::BadRequest("protocol is required".to_string()))?;
    if protocol != "openai" && protocol != "anthropic" && protocol != OPENAI_OAUTH_PROTOCOL {
        return Err(AppError::BadRequest(format!(
            "invalid protocol: {protocol}"
        )));
    }
    if !defaults
        .iter()
        .any(|endpoint| endpoint.protocol == protocol)
    {
        return Err(AppError::BadRequest(format!(
            "provider {provider_code} does not support protocol {protocol}"
        )));
    }
    let base_url = req
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::BadRequest("base_url is required".to_string()))?;
    if base_url.is_empty() {
        return Err(AppError::BadRequest("base_url is required".to_string()));
    }

    if req.use_credentials && provider_code == "openai" && protocol == OPENAI_OAUTH_PROTOCOL {
        let models = openai_oauth_catalog_models(&state).await?;
        if models.is_empty() {
            return Err(AppError::BadRequest("no models returned".to_string()));
        }
        record_provider_models(&state, provider_code, &models, "upstream", false).await?;
        return Ok(Json(FetchUpstreamModelsResponse { models }));
    }

    let channel_secret;
    let secret = if let Some(secret) = secret {
        secret
    } else if req.use_credentials {
        channel_secret = runtime_secret_from_enabled_credential(&state, provider_code).await?;
        channel_secret.as_str()
    } else if let Some(channel_id) = req.channel_id {
        channel_secret = upstream_model_secret_from_channel(&state, channel_id).await?;
        channel_secret.as_str()
    } else {
        return Err(AppError::BadRequest(
            "upstream api key is required".to_string(),
        ));
    };

    let models = fetch_upstream_models(&state, &protocol, base_url, secret).await?;
    if models.is_empty() {
        return Err(AppError::BadRequest("no models returned".to_string()));
    }
    record_provider_models(&state, provider_code, &models, "upstream", false).await?;
    Ok(Json(FetchUpstreamModelsResponse { models }))
}

async fn openai_oauth_catalog_models(state: &AppState) -> AppResult<Vec<String>> {
    let rows = sqlx::query(
        "SELECT pm.model
         FROM provider_model pm
         JOIN provider p ON p.code = pm.provider
         WHERE pm.provider = 'openai'
           AND pm.enabled = TRUE
           AND NOT pm.model = ANY(p.default_models)
         ORDER BY pm.model ASC",
    )
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter()
        .map(|row| row.try_get("model"))
        .collect::<Result<_, _>>()
        .map_err(Into::into)
}

async fn upstream_model_secret_from_channel(
    state: &AppState,
    channel_id: DbId,
) -> AppResult<String> {
    let channel_row = sqlx::query("SELECT provider, use_credentials FROM channel WHERE id = $1")
        .bind(channel_id)
        .fetch_optional(&state.db.pool)
        .await?
        .ok_or(AppError::NotFound)?;
    let provider: String = channel_row.try_get("provider")?;
    let use_credentials: bool = channel_row.try_get("use_credentials")?;
    if use_credentials {
        return runtime_secret_from_enabled_credential(state, &provider).await;
    }

    let row = sqlx::query(
        "SELECT id, secret_ciphertext
         FROM channel_key
         WHERE channel_id = $1 AND enabled = true
         ORDER BY healthy DESC, last_used_at DESC NULLS LAST, created_at ASC
         LIMIT 1",
    )
    .bind(channel_id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or_else(|| AppError::BadRequest("这个上游服务没有可用的上游 Key".to_string()))?;

    let key_id: DbId = row.try_get("id")?;
    let secret_ciphertext: String = row.try_get("secret_ciphertext")?;
    Ok(state.secrets.plaintext(key_id, &secret_ciphertext)?)
}

pub(crate) async fn fetch_upstream_models(
    state: &AppState,
    protocol: &str,
    base_url: &str,
    secret: &str,
) -> AppResult<Vec<String>> {
    let url = crate::relay::upstream_url(base_url, "/v1/models");
    let mut request = state.http.get(url);

    if protocol == "anthropic" {
        request = request
            .header("x-api-key", secret)
            .header("anthropic-version", &state.config.anthropic_version);
    } else {
        request = request.bearer_auth(secret);
    }

    let response = request.send().await?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::BadRequest(upstream_models_error_message(
            status.as_u16(),
        )));
    }

    let value = response.json::<Value>().await?;
    let models = extract_model_ids(&value);
    if models.is_empty() {
        return Err(AppError::BadRequest("no models returned".to_string()));
    }

    Ok(models)
}

fn extract_model_ids(value: &Value) -> Vec<String> {
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.as_array());
    let Some(items) = data else {
        return Vec::new();
    };

    let mut models = Vec::new();
    for item in items {
        let id = item
            .get("id")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str);
        let Some(id) = id.map(str::trim).filter(|id| !id.is_empty()) else {
            continue;
        };
        if !models.iter().any(|model| model == id) {
            models.push(id.to_string());
        }
    }

    models
}

fn upstream_models_error_message(status: u16) -> String {
    match status {
        401 | 403 => "API 密钥无效或无权限，请检查后重试".to_string(),
        404 => "Base URL 不正确，未找到模型列表接口".to_string(),
        429 => "上游请求过于频繁，请稍后重试".to_string(),
        500..=599 => "上游服务暂时不可用，请稍后重试".to_string(),
        _ => "获取模型列表失败，请检查 Base URL 和 API 密钥".to_string(),
    }
}

async fn provider_prices(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<Vec<ProviderPriceRecord>>> {
    Ok(Json(list_provider_prices(&state).await?))
}

async fn provider_models(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<Vec<ProviderModelRecord>>> {
    Ok(Json(list_provider_models(&state).await?))
}

async fn pricing_templates(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<Vec<PricingTemplateRecord>>> {
    Ok(Json(list_pricing_templates(&state).await?))
}

async fn smtp_setting(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<SmtpSettingRecord>> {
    Ok(Json(get_smtp_setting(&state).await?))
}

async fn upsert_smtp_setting_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<UpsertSmtpSettingRequest>,
) -> AppResult<Json<SmtpSettingRecord>> {
    Ok(Json(upsert_smtp_setting(&state, req).await?))
}

async fn test_smtp_setting_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<UpsertSmtpSettingRequest>,
) -> AppResult<Json<TestSmtpSettingResponse>> {
    Ok(Json(test_smtp_setting(&state, req).await?))
}

async fn payment_setting(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<PaymentSettingRecord>> {
    Ok(Json(get_payment_setting(&state).await?))
}

async fn upsert_payment_setting_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<UpsertPaymentSettingRequest>,
) -> AppResult<Json<PaymentSettingRecord>> {
    Ok(Json(upsert_payment_setting(&state, req).await?))
}

#[derive(Debug, Deserialize)]
struct UpdateAdminPasswordRequest {
    current_password: String,
    new_password: String,
}

async fn update_admin_password_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<UpdateAdminPasswordRequest>,
) -> AppResult<Json<Value>> {
    if req.current_password.is_empty() {
        return Err(AppError::BadRequest(
            "current password is required".to_string(),
        ));
    }
    auth::validate_user_password_input(&req.new_password)?;

    let rows = sqlx::query(
        r#"
        SELECT id, password_hash
        FROM admin
        WHERE status = 'enabled'
        ORDER BY id ASC
        "#,
    )
    .fetch_all(&state.db.pool)
    .await?;

    let Some(admin_id) = rows.iter().find_map(|row| {
        let id: DbId = row.try_get("id").ok()?;
        let password_hash: String = row.try_get("password_hash").ok()?;
        auth::verify_user_password(
            &req.current_password,
            &state.config.admin_token_secret,
            &password_hash,
        )
        .then_some(id)
    }) else {
        return Err(AppError::BadRequest(
            "current password is incorrect".to_string(),
        ));
    };

    let password_hash =
        auth::hash_user_password(&req.new_password, &state.config.admin_token_secret);
    sqlx::query(
        r#"
        UPDATE admin
        SET password_hash = $2,
            failed_login_attempts = 0,
            locked_until = NULL,
            password_changed_at = now(),
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(admin_id)
    .bind(password_hash)
    .execute(&state.db.pool)
    .await?;

    Ok(Json(json!({ "ok": true })))
}

async fn sync_pricing_templates_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<SyncPricingTemplatesRequest>,
) -> AppResult<Json<PricingTemplateSyncResult>> {
    Ok(Json(sync_pricing_templates(&state, req).await?))
}

async fn pricing_policies(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<Vec<PricingPolicyRecord>>> {
    Ok(Json(list_pricing_policies(&state).await?))
}

async fn upsert_provider_price_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<UpsertProviderPriceRequest>,
) -> AppResult<Json<ProviderPriceRecord>> {
    let price = upsert_provider_price(&state, req).await?;
    invalidate_cache(
        &state,
        InvalidationEvent::Price {
            provider: price.provider.clone(),
            model: price.model.clone(),
        },
    )
    .await;
    Ok(Json(price))
}

async fn upsert_pricing_policy_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<UpsertPricingPolicyRequest>,
) -> AppResult<Json<PricingPolicyRecord>> {
    let policy = upsert_pricing_policy(&state, req).await?;
    state.billing.invalidate_all_prices();
    Ok(Json(policy))
}

async fn create_channel_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<CreateChannelRequest>,
) -> AppResult<Json<ChannelRecord>> {
    let channel = create_channel(&state, req).await?;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(channel))
}

async fn update_channel_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
    Json(req): Json<UpdateChannelRequest>,
) -> AppResult<Json<ChannelRecord>> {
    let channel = update_channel(&state, id, req).await?;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(channel))
}

async fn delete_channel_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<Value>> {
    delete_channel(&state, id).await?;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(json!({ "ok": true })))
}

async fn channel_keys(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(channel_id): Path<DbId>,
) -> AppResult<Json<Vec<ChannelKeyRecord>>> {
    Ok(Json(list_channel_keys(&state, channel_id).await?))
}

async fn all_channel_keys(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<Vec<ChannelKeyRecord>>> {
    Ok(Json(list_all_channel_keys(&state).await?))
}

#[derive(Debug, Deserialize)]
struct ListCredentialsQuery {
    provider: Option<String>,
}

async fn credentials(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(query): Query<ListCredentialsQuery>,
) -> AppResult<Json<Vec<CredentialRecord>>> {
    Ok(Json(list_credentials(&state, query.provider).await?))
}

async fn upload_credentials_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    multipart: Multipart,
) -> AppResult<Json<CredentialUploadResult>> {
    let result = upload_credentials(&state, multipart).await?;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(result))
}

async fn refresh_credential_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<CredentialRecord>> {
    let credential = refresh_credential(&state, id).await?;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(credential))
}

async fn credential_models_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<Vec<CredentialModelRecord>>> {
    Ok(Json(list_credential_models(&state, id).await?))
}

async fn reset_credential_model_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path((id, model)): Path<(DbId, String)>,
) -> AppResult<Json<Value>> {
    reset_credential_model(&state, id, &model).await?;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(json!({ "ok": true })))
}

async fn enable_credential_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<CredentialRecord>> {
    let credential = enable_credential(&state, id).await?;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(credential))
}

async fn disable_credential_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<CredentialRecord>> {
    let credential = disable_credential(&state, id).await?;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(credential))
}

async fn delete_credential_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<Value>> {
    delete_credential(&state, id).await?;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(json!({ "ok": true })))
}

async fn create_channel_key_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(channel_id): Path<DbId>,
    Json(req): Json<CreateChannelKeyRequest>,
) -> AppResult<Json<ChannelKeyRecord>> {
    let key = create_channel_key(&state, channel_id, req).await?;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(key))
}

async fn update_channel_key_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path((_channel_id, key_id)): Path<(DbId, DbId)>,
    Json(req): Json<UpdateChannelKeyRequest>,
) -> AppResult<Json<ChannelKeyRecord>> {
    let key = update_channel_key(&state, key_id, req).await?;
    invalidate_cache(&state, InvalidationEvent::ChannelKeySecret { id: key_id }).await;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(key))
}

async fn delete_channel_key_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path((_channel_id, key_id)): Path<(DbId, DbId)>,
) -> AppResult<Json<Value>> {
    delete_channel_key(&state, key_id).await?;
    invalidate_cache(&state, InvalidationEvent::ChannelKeySecret { id: key_id }).await;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(json!({ "ok": true })))
}

async fn invalidate_cache(state: &AppState, event: InvalidationEvent) {
    state.cache_invalidator.invalidate(state, event).await;
}

#[derive(Debug, Deserialize)]
struct ListUsageParams {
    page: Option<i64>,
    limit: Option<i64>,
    cursor: Option<String>,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    query: Option<String>,
    model: Option<String>,
    status: Option<String>,
}

#[derive(Debug, Serialize)]
struct UsagePage {
    items: Vec<UsageRecord>,
    total: i64,
    page: i64,
    limit: i64,
    next_cursor: Option<String>,
    has_more: bool,
}

#[derive(Debug, Serialize)]
struct UsageRecord {
    id: DbId,
    user_id: Option<DbId>,
    user_email: Option<String>,
    user_key_id: Option<DbId>,
    channel_id: Option<DbId>,
    channel_key_id: Option<DbId>,
    credential_id: Option<DbId>,
    provider: String,
    model: Option<String>,
    status_code: Option<i32>,
    streamed: bool,
    latency_ms: i64,
    first_response_ms: Option<i64>,
    output_tokens_per_second: Option<f64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    cache_in_tokens: Option<i64>,
    cache_create_in_tokens: Option<i64>,
    cache_create_5m_in_tokens: Option<i64>,
    cache_create_1h_in_tokens: Option<i64>,
    reason_out_tokens: Option<i64>,
    audio_in_tokens: Option<i64>,
    audio_out_tokens: Option<i64>,
    cost_micro_usd: Option<i64>,
    billing_status: String,
    error_summary: Option<String>,
    created_at: DateTime<Utc>,
}

async fn usage(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(params): Query<ListUsageParams>,
) -> AppResult<Json<UsagePage>> {
    let page = params.page.unwrap_or(1).max(1);
    let limit = params.limit.unwrap_or(20).clamp(1, 500);
    let start = params.start.clone();
    let end = params.end.clone();
    let query = params
        .query
        .as_deref()
        .or(params.model.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let status = match params.status.as_deref() {
        Some("success") => Some("success"),
        Some("failed") => Some("failed"),
        _ => None,
    };
    let (cursor_created_at, cursor_id) = parse_usage_cursor(params.cursor.as_deref())?
        .map(|cursor| (Some(cursor.0), Some(cursor.1)))
        .unwrap_or((None, None));
    let query_pattern = query.as_deref().map(|value| format!("%{value}%"));
    let query_pattern = query_pattern.as_deref();
    let rows = sqlx::query(
        r#"SELECT usage_record.id, usage_record.user_id, u.email::text AS user_email,
                usage_record.user_key_id, usage_record.channel_id, usage_record.channel_key_id,
                usage_record.credential_id, usage_record.provider, usage_record.model,
                usage_record.status_code, usage_record.streamed, usage_record.latency_ms,
                usage_record.first_response_ms, usage_record.output_tokens_per_second,
                usage_record.input_tokens, usage_record.output_tokens, usage_record.total_tokens,
                usage_record.cache_in_tokens, usage_record.cache_create_in_tokens,
                usage_record.cache_create_5m_in_tokens, usage_record.cache_create_1h_in_tokens,
                usage_record.reason_out_tokens, usage_record.audio_in_tokens,
                usage_record.audio_out_tokens, usage_record.cost_micro_usd,
                usage_record.billing_status, usage_record.error_summary, usage_record.created_at
         FROM usage AS usage_record
         LEFT JOIN "user" u ON u.id = usage_record.user_id
         WHERE ($1::timestamptz IS NULL OR usage_record.created_at >= $1)
           AND ($2::timestamptz IS NULL OR usage_record.created_at <= $2)
           AND (
             $3::text IS NULL
             OR usage_record.provider ILIKE $3
             OR usage_record.model ILIKE $3
             OR usage_record.user_id::text ILIKE $3
             OR u.email::text ILIKE $3
           )
           AND (
             $4::text IS NULL
             OR ($4 = 'success' AND usage_record.status_code >= 200 AND usage_record.status_code < 400)
             OR ($4 = 'failed' AND (usage_record.status_code >= 400 OR usage_record.error_summary IS NOT NULL))
           )
           AND ($5::timestamptz IS NULL OR (usage_record.created_at, usage_record.id) < ($5, $6))
         ORDER BY usage_record.created_at DESC, usage_record.id DESC
         LIMIT $7"#,
    )
    .bind(start)
    .bind(end)
    .bind(query_pattern)
    .bind(status)
    .bind(cursor_created_at)
    .bind(cursor_id)
    .bind(limit + 1)
    .fetch_all(&state.db.pool)
    .await?;
    let has_more = rows.len() > limit as usize;
    let rows = rows.into_iter().take(limit as usize).collect::<Vec<_>>();
    let next_cursor = has_more
        .then(|| rows.last())
        .flatten()
        .map(usage_cursor_from_row)
        .transpose()?;
    Ok(Json(UsagePage {
        items: rows.iter().map(usage_from_row).collect::<Result<_, _>>()?,
        total: rows.len() as i64,
        page,
        limit,
        next_cursor,
        has_more,
    }))
}

fn parse_usage_cursor(cursor: Option<&str>) -> AppResult<Option<(DateTime<Utc>, DbId)>> {
    let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let Some((created_at, id)) = cursor.rsplit_once('|') else {
        return Err(AppError::BadRequest("invalid usage cursor".to_string()));
    };
    let created_at = DateTime::parse_from_rfc3339(created_at)
        .map_err(|_| AppError::BadRequest("invalid usage cursor".to_string()))?
        .with_timezone(&Utc);
    let id = id
        .parse::<DbId>()
        .map_err(|_| AppError::BadRequest("invalid usage cursor".to_string()))?;
    Ok(Some((created_at, id)))
}

fn usage_cursor_from_row(row: &sqlx::postgres::PgRow) -> Result<String, sqlx::Error> {
    let created_at: DateTime<Utc> = row.try_get("created_at")?;
    let id: DbId = row.try_get("id")?;
    Ok(format!("{}|{}", created_at.to_rfc3339(), id))
}

async fn health(State(state): State<Arc<AppState>>, _admin: AdminAuth) -> AppResult<Json<Value>> {
    let db_ok = sqlx::query("SELECT 1")
        .execute(&state.db.pool)
        .await
        .is_ok();
    let row = sqlx::query(
        r#"
        SELECT
            (SELECT count(*) FROM "user") AS users,
            (SELECT count(*) FROM user_key) AS user_keys,
            (SELECT count(*) FROM channel) AS channels,
            (SELECT count(*) FROM channel_key) AS channel_keys,
            (SELECT count(*) FROM usage) AS usage
        "#,
    )
    .fetch_one(&state.db.pool)
    .await?;

    Ok(Json(json!({
        "database": db_ok,
        "users": row.try_get::<i64, _>("users")?,
        "user_keys": row.try_get::<i64, _>("user_keys")?,
        "channels": row.try_get::<i64, _>("channels")?,
        "channel_keys": row.try_get::<i64, _>("channel_keys")?,
        "usage": row.try_get::<i64, _>("usage")?
    })))
}

fn usage_from_row(row: &sqlx::postgres::PgRow) -> Result<UsageRecord, sqlx::Error> {
    Ok(UsageRecord {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        user_email: row.try_get("user_email")?,
        user_key_id: row.try_get("user_key_id")?,
        channel_id: row.try_get("channel_id")?,
        channel_key_id: row.try_get("channel_key_id")?,
        credential_id: row.try_get("credential_id")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        status_code: row.try_get("status_code")?,
        streamed: row.try_get("streamed")?,
        latency_ms: row.try_get("latency_ms")?,
        first_response_ms: row.try_get("first_response_ms")?,
        output_tokens_per_second: row.try_get("output_tokens_per_second")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        cache_in_tokens: row.try_get("cache_in_tokens")?,
        cache_create_in_tokens: row.try_get("cache_create_in_tokens")?,
        cache_create_5m_in_tokens: row.try_get("cache_create_5m_in_tokens")?,
        cache_create_1h_in_tokens: row.try_get("cache_create_1h_in_tokens")?,
        reason_out_tokens: row.try_get("reason_out_tokens")?,
        audio_in_tokens: row.try_get("audio_in_tokens")?,
        audio_out_tokens: row.try_get("audio_out_tokens")?,
        cost_micro_usd: row.try_get("cost_micro_usd")?,
        billing_status: row.try_get("billing_status")?,
        error_summary: row.try_get("error_summary")?,
        created_at: row.try_get("created_at")?,
    })
}
