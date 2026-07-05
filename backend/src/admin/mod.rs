pub(crate) mod apps;
pub(crate) mod channel;
pub(crate) mod credentials;
pub(crate) mod diagnostics;
mod openai;
pub(crate) mod price;
pub(crate) mod project;
pub(crate) mod provider;
pub(crate) mod setting;
mod upstream;
mod usage_stats;
mod user;
pub(crate) mod version;

use std::{convert::Infallible, sync::Arc};

use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, HeaderValue},
    response::sse::{Event, KeepAlive, Sse},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::{self, AdminAuth},
    billing::CreditAccountType,
    cache::InvalidationEvent,
    error::{AppError, AppResult},
    id::DbId,
    input::{bounded_limit, trimmed_non_empty},
    pagination::{created_id_cursor_page, parse_created_id_cursor},
    project::models::{
        auto_configure_project_model, create_project_model, delete_project_model,
        list_project_models, update_project_model, AutoConfigureProjectModelRequest,
        AutoConfigureResponse, ProjectModelRecord, UpdateProjectModelRequest,
        UpsertProjectModelRequest, UsageRoutingCandidateSummary,
    },
    AppState,
};

use self::{
    channel::{
        create_channel, create_channel_key, delete_channel, delete_channel_key,
        list_all_channel_keys, list_channel_keys, list_channels, reveal_channel_key_secret,
        update_channel, update_channel_key, update_channel_model, ChannelKeyRecord,
        ChannelModelRecord, ChannelRecord, CreateChannelKeyRequest, CreateChannelRequest,
        UpdateChannelKeyRequest, UpdateChannelModelRequest, UpdateChannelRequest,
    },
    credentials::{
        delete_credential, disable_credential, enable_credential, list_credential_models,
        list_credentials, refresh_credential, reset_credential_model, upload_credentials,
        CredentialModelRecord, CredentialRecord, CredentialUploadResult,
    },
    diagnostics::{
        diagnose_channel, diagnose_channel_with_progress, ChannelDiagnosticEvent,
        ChannelDiagnosticReport,
    },
    price::{
        list_model_reference_catalog, list_pricing_policies, list_pricing_templates,
        list_provider_models, list_provider_prices, live_model_reference_catalog,
        sync_pricing_templates, upsert_pricing_policy, upsert_provider_price,
        ModelReferenceCatalogRecord, PricingPolicyRecord, PricingTemplateRecord,
        PricingTemplateSyncResult, ProviderModelRecord, ProviderPriceRecord,
        SyncPricingTemplatesRequest, UpsertPricingPolicyRequest, UpsertProviderPriceRequest,
    },
    project::{
        add_project_member, create_project, delete_project, delete_project_member,
        list_project_members, list_projects, update_project, update_project_member,
        CreateProjectRequest, CreatedProject, CreatedProjectMember, ListProjectsQuery,
        ProjectMemberRecord, ProjectPage, ProjectRecord, UpdateProjectMemberRequest,
        UpdateProjectRequest, UpsertProjectMemberRequest,
    },
    provider::{
        ensure_builtin_manual_provider_by_code, list_providers, provider_default_endpoints,
        record_provider_models, ProviderRecord, OPENAI_OAUTH_PROTOCOL,
    },
    setting::{
        ensure_default_text_model_setting, get_admin_model_setting, get_site_setting,
        get_smtp_setting, test_smtp_setting, upsert_admin_model_setting, upsert_site_setting,
        upsert_smtp_setting, AdminModelSettingRecord, SiteSettingRecord, SmtpSettingRecord,
        TestSmtpSettingResponse, UpsertAdminModelSettingRequest, UpsertSiteSettingRequest,
        UpsertSiteSettingResponse, UpsertSmtpSettingRequest,
    },
    upstream::upstream_models,
    user::{
        adjust_credit, adjust_default_project_credit, adjust_user_key_model_credit, create_user,
        create_user_key, delete_user, delete_user_key, list_user_groups, list_user_keys,
        list_users, update_user, update_user_key, CreateUserKeyRequest, CreateUserRequest,
        CreatedUserKey, ListUserKeysQuery, ListUsersQuery, UpdateUserKeyRequest, UpdateUserRequest,
        UserGroupRecord, UserKeyModelCreditRecord, UserKeyPage, UserKeyRecord, UserPage,
        UserRecord,
    },
    version::{check_latest_version, VersionCheckResponse},
};
use crate::payment::settings::{
    get_payment_setting, upsert_payment_setting, PaymentSettingRecord, UpsertPaymentSettingRequest,
};

const ADMIN_RUNTIME_RESTART_DELAY: std::time::Duration = std::time::Duration::from_secs(2);
const USAGE_EXPORT_LIMIT: i64 = 100_000;

pub(crate) use upstream::fetch_upstream_models;

pub fn public_router() -> Router<Arc<AppState>> {
    user::public_router().route("/api/public/site", get(public_site_setting))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .merge(apps::router())
        .merge(usage_stats::router())
        .route("/api/admin/users", get(users).post(create_user_handler))
        .route("/api/admin/user-groups", get(user_groups))
        .route(
            "/api/admin/users/{id}",
            patch(update_user_handler).delete(delete_user_handler),
        )
        .route(
            "/api/admin/users/{id}/default-project-credit",
            post(adjust_default_project_credit_handler),
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
        .route(
            "/api/admin/projects",
            get(projects).post(create_project_handler),
        )
        .route(
            "/api/admin/projects/{id}",
            patch(update_project_handler).delete(delete_project_handler),
        )
        .route(
            "/api/admin/projects/{id}/members",
            get(project_members).post(add_project_member_handler),
        )
        .route(
            "/api/admin/projects/{id}/models",
            get(project_models_handler).post(create_project_model_handler),
        )
        .route(
            "/api/admin/projects/{id}/models/auto-configure",
            post(auto_configure_project_model_handler),
        )
        .route(
            "/api/admin/projects/{id}/models/{model}",
            patch(update_project_model_handler).delete(delete_project_model_handler),
        )
        .route(
            "/api/admin/projects/{id}/members/{member_id}",
            patch(update_project_member_handler).delete(delete_project_member_handler),
        )
        .route("/api/admin/credits", post(adjust_credit_handler))
        .route(
            "/api/admin/channels",
            get(channels).post(create_channel_handler),
        )
        .route("/api/admin/providers", get(providers))
        .route("/api/admin/provider-models", get(provider_models))
        .route("/api/admin/upstream-models", post(upstream_models))
        .route(
            "/api/admin/model-reference-catalog",
            get(model_reference_catalog),
        )
        .route(
            "/api/admin/model-reference-catalog/live",
            get(live_model_reference_catalog_handler),
        )
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
            "/api/admin/settings/site",
            get(site_setting).post(upsert_site_setting_handler),
        )
        .route(
            "/api/admin/settings/admin-model",
            get(admin_model_setting).post(upsert_admin_model_setting_handler),
        )
        .route(
            "/api/admin/settings/admin-password",
            post(update_admin_password_handler),
        )
        .route("/api/admin/settings/version", get(version_check_handler))
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
            "/api/admin/channels/{id}/models/{model}",
            patch(update_channel_model_handler),
        )
        .route(
            "/api/admin/channels/{id}/diagnose",
            post(diagnose_channel_handler),
        )
        .route(
            "/api/admin/channels/{id}/diagnose/stream",
            post(diagnose_channel_stream_handler),
        )
        .route(
            "/api/admin/channels/{id}/keys",
            get(channel_keys).post(create_channel_key_handler),
        )
        .route(
            "/api/admin/channels/{id}/keys/{key_id}",
            patch(update_channel_key_handler).delete(delete_channel_key_handler),
        )
        .route(
            "/api/admin/channels/{id}/keys/{key_id}/secret",
            get(reveal_channel_key_secret_handler),
        )
        .route("/api/admin/usage", get(usage))
        .route("/api/admin/usage/export.csv", get(export_usage_csv))
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
    amount_micros: i64,
    #[serde(default = "default_credit_reason")]
    reason: String,
}

#[derive(Debug, Deserialize)]
struct AdjustDefaultProjectCreditRequest {
    amount_micros: i64,
    #[serde(default = "default_credit_reason")]
    reason: String,
}

#[derive(Debug, Deserialize)]
struct AdjustUserKeyModelCreditRequest {
    user_key_id: DbId,
    model: String,
    amount_micros: i64,
    #[serde(default = "default_credit_reason")]
    reason: String,
}

#[derive(Debug, Serialize)]
struct AdjustCreditResponse {
    balance_micros: i64,
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
        "project" => CreditAccountType::Project,
        "user_key" => CreditAccountType::UserKey,
        "user_key_model" => CreditAccountType::UserKeyModel,
        other => {
            return Err(AppError::BadRequest(format!(
                "invalid credit_account type: {other}"
            )));
        }
    };
    let balance_micros = adjust_credit(
        &state,
        credit_account_type,
        req.owner_id,
        req.amount_micros,
        &req.reason,
    )
    .await?;
    Ok(Json(AdjustCreditResponse { balance_micros }))
}

async fn adjust_default_project_credit_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
    Json(req): Json<AdjustDefaultProjectCreditRequest>,
) -> AppResult<Json<AdjustCreditResponse>> {
    let balance_micros =
        adjust_default_project_credit(&state, id, req.amount_micros, &req.reason).await?;
    Ok(Json(AdjustCreditResponse { balance_micros }))
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
        req.amount_micros,
        &req.reason,
    )
    .await?;
    Ok(Json(record))
}

async fn projects(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListProjectsQuery>,
    _admin: AdminAuth,
) -> AppResult<Json<ProjectPage>> {
    Ok(Json(list_projects(&state, query).await?))
}

async fn create_project_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<CreateProjectRequest>,
) -> AppResult<Json<CreatedProject>> {
    Ok(Json(create_project(&state, req).await?))
}

async fn update_project_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
    Json(req): Json<UpdateProjectRequest>,
) -> AppResult<Json<ProjectRecord>> {
    Ok(Json(update_project(&state, id, req).await?))
}

async fn delete_project_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<Value>> {
    delete_project(&state, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn project_members(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<Vec<ProjectMemberRecord>>> {
    Ok(Json(list_project_members(&state, id).await?))
}

async fn add_project_member_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
    Json(req): Json<UpsertProjectMemberRequest>,
) -> AppResult<Json<CreatedProjectMember>> {
    Ok(Json(add_project_member(&state, id, req).await?))
}

async fn update_project_member_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path((id, member_id)): Path<(DbId, DbId)>,
    Json(req): Json<UpdateProjectMemberRequest>,
) -> AppResult<Json<ProjectMemberRecord>> {
    Ok(Json(
        update_project_member(&state, id, member_id, req).await?,
    ))
}

async fn delete_project_member_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path((id, member_id)): Path<(DbId, DbId)>,
) -> AppResult<Json<Value>> {
    delete_project_member(&state, id, member_id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn project_models_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<Vec<ProjectModelRecord>>> {
    Ok(Json(list_project_models(&state.db.pool, id).await?))
}

async fn create_project_model_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
    Json(req): Json<UpsertProjectModelRequest>,
) -> AppResult<Json<ProjectModelRecord>> {
    let record = create_project_model(&state.db.pool, id, req).await?;
    invalidate_project_auth(&state, id).await?;
    Ok(Json(record))
}

async fn auto_configure_project_model_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
    Json(req): Json<AutoConfigureProjectModelRequest>,
) -> AppResult<Json<AutoConfigureResponse>> {
    Ok(Json(auto_configure_project_model(&state, id, req).await?))
}

async fn update_project_model_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path((id, model)): Path<(DbId, String)>,
    Json(req): Json<UpdateProjectModelRequest>,
) -> AppResult<Json<ProjectModelRecord>> {
    let record = update_project_model(&state.db.pool, id, &model, req).await?;
    invalidate_project_auth(&state, id).await?;
    Ok(Json(record))
}

async fn delete_project_model_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path((id, model)): Path<(DbId, String)>,
) -> AppResult<Json<Value>> {
    delete_project_model(&state.db.pool, id, &model).await?;
    invalidate_project_auth(&state, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn invalidate_project_auth(state: &AppState, project_id: DbId) -> AppResult<()> {
    let rows = sqlx::query("SELECT id FROM user_key WHERE project_id = $1")
        .bind(project_id)
        .fetch_all(&state.db.pool)
        .await?;
    for row in rows {
        let id: DbId = row.try_get("id")?;
        state
            .cache_invalidator
            .invalidate(state, InvalidationEvent::UserKey { id })
            .await;
    }
    Ok(())
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

async fn model_reference_catalog(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<Vec<ModelReferenceCatalogRecord>>> {
    Ok(Json(list_model_reference_catalog(&state).await?))
}

async fn live_model_reference_catalog_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<Vec<ModelReferenceCatalogRecord>>> {
    Ok(Json(live_model_reference_catalog(&state).await?))
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

async fn site_setting(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<SiteSettingRecord>> {
    Ok(Json(get_site_setting(&state).await?))
}

async fn upsert_site_setting_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<UpsertSiteSettingRequest>,
) -> AppResult<Json<UpsertSiteSettingResponse>> {
    let result = upsert_site_setting(&state, req).await?;
    if result.restart_required {
        schedule_admin_runtime_restart(state.runtime_restart_tx.clone());
    }
    Ok(Json(result))
}

async fn admin_model_setting(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<AdminModelSettingRecord>> {
    Ok(Json(get_admin_model_setting(&state).await?))
}

async fn upsert_admin_model_setting_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<UpsertAdminModelSettingRequest>,
) -> AppResult<Json<AdminModelSettingRecord>> {
    Ok(Json(upsert_admin_model_setting(&state, req).await?))
}

async fn public_site_setting(
    State(state): State<Arc<AppState>>,
) -> AppResult<Json<SiteSettingRecord>> {
    Ok(Json(get_site_setting(&state).await?))
}

fn schedule_admin_runtime_restart(restart_tx: tokio::sync::watch::Sender<bool>) {
    tokio::spawn(async move {
        tokio::time::sleep(ADMIN_RUNTIME_RESTART_DELAY).await;
        let _ = restart_tx.send(true);
    });
}

#[derive(Debug, Deserialize)]
struct UpdateAdminPasswordRequest {
    current_password: String,
    new_password: String,
}

async fn update_admin_password_handler(
    State(state): State<Arc<AppState>>,
    admin: AdminAuth,
    Json(req): Json<UpdateAdminPasswordRequest>,
) -> AppResult<Json<Value>> {
    if req.current_password.is_empty() {
        return Err(AppError::BadRequest(
            "current password is required".to_string(),
        ));
    }
    auth::validate_user_password_input(&req.new_password)?;

    let row = sqlx::query(
        r#"
        SELECT password_hash
        FROM admin
        WHERE id = $1 AND status = 'enabled'
        "#,
    )
    .bind(admin.admin_id)
    .fetch_optional(&state.db.pool)
    .await?;

    let Some(row) = row else {
        return Err(AppError::Unauthorized);
    };
    let password_hash: String = row.try_get("password_hash")?;
    if !auth::verify_user_password(
        &req.current_password,
        &state.config.admin_token_secret,
        &password_hash,
    ) {
        return Err(AppError::BadRequest(
            "current password is incorrect".to_string(),
        ));
    }

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
    .bind(admin.admin_id)
    .bind(password_hash)
    .execute(&state.db.pool)
    .await?;

    Ok(Json(json!({ "ok": true })))
}

async fn version_check_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<VersionCheckResponse>> {
    Ok(Json(check_latest_version(&state).await?))
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
    ensure_default_text_model_setting(&state).await?;
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
    ensure_default_text_model_setting(&state).await?;
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
    ensure_default_text_model_setting(&state).await?;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(channel))
}

async fn update_channel_model_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path((id, model)): Path<(DbId, String)>,
    Json(req): Json<UpdateChannelModelRequest>,
) -> AppResult<Json<ChannelModelRecord>> {
    let model = update_channel_model(&state, id, &model, req).await?;
    ensure_default_text_model_setting(&state).await?;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(model))
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

async fn diagnose_channel_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<ChannelDiagnosticReport>> {
    let report = diagnose_channel(&state, id).await?;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(report))
}

async fn diagnose_channel_stream_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ChannelDiagnosticEvent>();
    tokio::spawn(async move {
        match diagnose_channel_with_progress(&state, id, Some(tx.clone())).await {
            Ok(_) => {
                invalidate_cache(&state, InvalidationEvent::Routing).await;
            }
            Err(err) => {
                let _ = tx.send(ChannelDiagnosticEvent::Error {
                    message: err.to_string(),
                });
            }
        }
    });

    let stream = futures_util::stream::unfold(rx, |mut rx| async {
        let event = rx.recv().await?;
        let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
        Some((
            Ok::<_, Infallible>(Event::default().event(event.event_name()).data(data)),
            rx,
        ))
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
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
    ensure_default_text_model_setting(&state).await?;
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
    ensure_default_text_model_setting(&state).await?;
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
    ensure_default_text_model_setting(&state).await?;
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
    ensure_default_text_model_setting(&state).await?;
    invalidate_cache(&state, InvalidationEvent::ChannelKeySecret { id: key_id }).await;
    invalidate_cache(&state, InvalidationEvent::Routing).await;
    Ok(Json(key))
}

async fn reveal_channel_key_secret_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path((channel_id, key_id)): Path<(DbId, DbId)>,
) -> AppResult<Json<Value>> {
    let secret = reveal_channel_key_secret(&state, channel_id, key_id).await?;
    Ok(Json(json!({ "secret": secret })))
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
    user_username: Option<String>,
    user_key_id: Option<DbId>,
    channel_id: Option<DbId>,
    channel_name: Option<String>,
    channel_key_id: Option<DbId>,
    credential_id: Option<DbId>,
    relay_trace_id: Option<Uuid>,
    relay_attempt: i32,
    relay_final: bool,
    relay_path: Option<String>,
    relay_path_index: Option<i32>,
    model: Option<String>,
    upstream_model: Option<String>,
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
    billing_meter: String,
    billable_units: i64,
    cost_micros: Option<i64>,
    billing_status: String,
    error_summary: Option<String>,
    routing: Option<UsageRouting>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct UsageRouting {
    id: DbId,
    project_id: DbId,
    project_model_id: Option<DbId>,
    requested_model: String,
    selected_model: String,
    selected_channel_id: Option<DbId>,
    decision_source: String,
    tier: String,
    task_type: String,
    confidence: f64,
    reason_code: String,
    matched_rule_ids: Vec<String>,
    candidate_summary: Vec<UsageRoutingCandidateSummary>,
    fallback_reason: Option<String>,
    classifier_model: Option<String>,
    latency_ms: i64,
    created_at: DateTime<Utc>,
}

async fn usage(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(params): Query<ListUsageParams>,
) -> AppResult<Json<UsagePage>> {
    let page = params.page.unwrap_or(1).max(1);
    let limit = bounded_limit(params.limit, 20, 500);
    let cursor = parse_created_id_cursor(params.cursor.as_deref(), "invalid usage cursor")?;
    let rows = usage_rows(&state, &params, limit + 1, cursor).await?;
    let (rows, next_cursor, has_more) = created_id_cursor_page(rows, limit)?;
    Ok(Json(UsagePage {
        items: rows.iter().map(usage_from_row).collect::<Result<_, _>>()?,
        total: rows.len() as i64,
        page,
        limit,
        next_cursor,
        has_more,
    }))
}

async fn export_usage_csv(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(params): Query<ListUsageParams>,
) -> AppResult<Response> {
    let rows = usage_rows(&state, &params, USAGE_EXPORT_LIMIT + 1, None).await?;
    if rows.len() > USAGE_EXPORT_LIMIT as usize {
        return Err(AppError::BadRequestWithCode {
            code: "export_limit_exceeded",
            message: "export result exceeds 100000 rows; narrow the filters",
        });
    }

    let records = rows
        .iter()
        .map(usage_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    csv_response(
        &usage_export_filename(params.start, params.end),
        usage_csv_rows(records),
    )
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

async fn usage_rows(
    state: &AppState,
    params: &ListUsageParams,
    limit: i64,
    cursor: Option<(DateTime<Utc>, DbId)>,
) -> AppResult<Vec<sqlx::postgres::PgRow>> {
    let query =
        trimmed_non_empty(params.query.as_deref().or(params.model.as_deref())).map(str::to_string);
    let status = match params.status.as_deref() {
        Some("success") => Some("success"),
        Some("failed") => Some("failed"),
        _ => None,
    };
    let (cursor_created_at, cursor_id) = cursor.map_or((None, None), |(created_at, id)| {
        (Some(created_at), Some(id))
    });
    let query_pattern = query.as_deref().map(|value| format!("%{value}%"));
    let query_pattern = query_pattern.as_deref();

    let rows = sqlx::query(
        r#"SELECT usage_record.id, usage_record.user_id, u.email::text AS user_email,
                u.username AS user_username,
                usage_record.user_key_id, usage_record.channel_id,
                current_channel.name AS channel_name, usage_record.channel_key_id,
                usage_record.credential_id, usage_record.relay_trace_id,
                usage_record.relay_attempt, usage_record.relay_final,
                usage_record.model, usage_record.upstream_model,
                usage_record.status_code, usage_record.streamed, usage_record.latency_ms,
                usage_record.first_response_ms, usage_record.output_tokens_per_second,
                usage_record.input_tokens, usage_record.output_tokens, usage_record.total_tokens,
                usage_record.cache_in_tokens, usage_record.cache_create_in_tokens,
                usage_record.cache_create_5m_in_tokens, usage_record.cache_create_1h_in_tokens,
                usage_record.reason_out_tokens, usage_record.audio_in_tokens,
                usage_record.audio_out_tokens, usage_record.billing_meter,
                usage_record.billable_units, usage_record.cost_micros,
                usage_record.billing_status, usage_record.error_summary, usage_record.created_at,
                usage_routing.id AS routing_id,
                usage_routing.project_id AS routing_project_id,
                usage_routing.project_model_id AS routing_project_model_id,
                usage_routing.requested_model AS routing_requested_model,
                usage_routing.selected_model AS routing_selected_model,
                usage_routing.selected_channel_id AS routing_selected_channel_id,
                usage_routing.decision_source AS routing_decision_source,
                usage_routing.tier AS routing_tier,
                usage_routing.task_type AS routing_task_type,
                usage_routing.confidence AS routing_confidence,
                usage_routing.reason_code AS routing_reason_code,
                usage_routing.matched_rule_ids AS routing_matched_rule_ids,
                usage_routing.candidate_summary AS routing_candidate_summary,
                usage_routing.fallback_reason AS routing_fallback_reason,
                usage_routing.classifier_model AS routing_classifier_model,
                usage_routing.latency_ms AS routing_latency_ms,
                usage_routing.created_at AS routing_created_at,
                rp.relay_path, rp.relay_path_index
         FROM usage AS usage_record
         LEFT JOIN "user" u ON u.id = usage_record.user_id
         LEFT JOIN channel current_channel ON current_channel.id = usage_record.channel_id
         LEFT JOIN usage_routing ON usage_routing.usage_id = usage_record.id
         LEFT JOIN LATERAL (
           SELECT
             string_agg('#' || sibling.channel_id::text, ' → '
                        ORDER BY sibling.relay_attempt ASC, sibling.id ASC) AS relay_path,
             (
               SELECT count(*)::int
               FROM usage prev
               WHERE prev.relay_trace_id = usage_record.relay_trace_id
                 AND (prev.relay_attempt, prev.id)
                     < (usage_record.relay_attempt, usage_record.id)
             ) AS relay_path_index
           FROM usage sibling
           WHERE sibling.relay_trace_id = usage_record.relay_trace_id
         ) rp ON usage_record.relay_trace_id IS NOT NULL
         WHERE ($1::timestamptz IS NULL OR usage_record.created_at >= $1)
           AND ($2::timestamptz IS NULL OR usage_record.created_at <= $2)
           AND (
             $3::text IS NULL
             OR usage_record.model ILIKE $3
             OR usage_record.upstream_model ILIKE $3
             OR usage_record.relay_trace_id::text ILIKE $3
             OR usage_record.user_id::text ILIKE $3
             OR u.email::text ILIKE $3
             OR current_channel.name ILIKE $3
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
    .bind(params.start)
    .bind(params.end)
    .bind(query_pattern)
    .bind(status)
    .bind(cursor_created_at)
    .bind(cursor_id)
    .bind(limit)
    .fetch_all(&state.db.pool)
    .await?;

    Ok(rows)
}

fn usage_from_row(row: &sqlx::postgres::PgRow) -> Result<UsageRecord, sqlx::Error> {
    Ok(UsageRecord {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        user_email: row.try_get("user_email")?,
        user_username: row.try_get("user_username")?,
        user_key_id: row.try_get("user_key_id")?,
        channel_id: row.try_get("channel_id")?,
        channel_name: row.try_get("channel_name")?,
        channel_key_id: row.try_get("channel_key_id")?,
        credential_id: row.try_get("credential_id")?,
        relay_trace_id: row.try_get("relay_trace_id")?,
        relay_attempt: row.try_get("relay_attempt")?,
        relay_final: row.try_get("relay_final")?,
        relay_path: row.try_get("relay_path")?,
        relay_path_index: row.try_get("relay_path_index")?,
        model: row.try_get("model")?,
        upstream_model: row.try_get("upstream_model")?,
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
        billing_meter: row.try_get("billing_meter")?,
        billable_units: row.try_get("billable_units")?,
        cost_micros: row.try_get("cost_micros")?,
        billing_status: row.try_get("billing_status")?,
        error_summary: row.try_get("error_summary")?,
        routing: usage_routing_from_row(row)?,
        created_at: row.try_get("created_at")?,
    })
}

fn usage_routing_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<Option<UsageRouting>, sqlx::Error> {
    let Some(id) = row.try_get::<Option<DbId>, _>("routing_id")? else {
        return Ok(None);
    };
    Ok(Some(UsageRouting {
        id,
        project_id: row.try_get("routing_project_id")?,
        project_model_id: row.try_get("routing_project_model_id")?,
        requested_model: row.try_get("routing_requested_model")?,
        selected_model: row.try_get("routing_selected_model")?,
        selected_channel_id: row.try_get("routing_selected_channel_id")?,
        decision_source: row.try_get("routing_decision_source")?,
        tier: row.try_get("routing_tier")?,
        task_type: row.try_get("routing_task_type")?,
        confidence: row.try_get("routing_confidence")?,
        reason_code: row.try_get("routing_reason_code")?,
        matched_rule_ids: row
            .try_get::<sqlx::types::Json<Vec<String>>, _>("routing_matched_rule_ids")?
            .0,
        candidate_summary: row
            .try_get::<sqlx::types::Json<Vec<UsageRoutingCandidateSummary>>, _>(
                "routing_candidate_summary",
            )?
            .0,
        fallback_reason: row.try_get("routing_fallback_reason")?,
        classifier_model: row.try_get("routing_classifier_model")?,
        latency_ms: row.try_get("routing_latency_ms")?,
        created_at: row.try_get("routing_created_at")?,
    }))
}

fn usage_csv_rows(records: Vec<UsageRecord>) -> Vec<Vec<String>> {
    let mut rows = vec![vec![
        "id".into(),
        "created_at".into(),
        "user_id".into(),
        "user_email".into(),
        "user_username".into(),
        "user_key_id".into(),
        "channel_id".into(),
        "channel_key_id".into(),
        "credential_id".into(),
        "relay_trace_id".into(),
        "relay_attempt".into(),
        "relay_final".into(),
        "relay_path".into(),
        "model".into(),
        "upstream_model".into(),
        "routing_requested_model".into(),
        "routing_selected_model".into(),
        "routing_tier".into(),
        "routing_task_type".into(),
        "routing_reason_code".into(),
        "routing_fallback_reason".into(),
        "status_code".into(),
        "streamed".into(),
        "latency_ms".into(),
        "first_response_ms".into(),
        "output_tokens_per_second".into(),
        "input_tokens".into(),
        "output_tokens".into(),
        "total_tokens".into(),
        "cache_in_tokens".into(),
        "cache_create_in_tokens".into(),
        "cache_create_5m_in_tokens".into(),
        "cache_create_1h_in_tokens".into(),
        "reason_out_tokens".into(),
        "audio_in_tokens".into(),
        "audio_out_tokens".into(),
        "billing_meter".into(),
        "billable_units".into(),
        "cost_micros".into(),
        "billing_status".into(),
        "error_summary".into(),
    ]];

    rows.extend(records.into_iter().map(|record| {
        vec![
            record.id.to_string(),
            record.created_at.to_rfc3339(),
            optional_id(record.user_id),
            record.user_email.unwrap_or_default(),
            record.user_username.unwrap_or_default(),
            optional_id(record.user_key_id),
            optional_id(record.channel_id),
            optional_id(record.channel_key_id),
            optional_id(record.credential_id),
            record
                .relay_trace_id
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record.relay_attempt.to_string(),
            record.relay_final.to_string(),
            record.relay_path.unwrap_or_default(),
            record.model.unwrap_or_default(),
            record.upstream_model.unwrap_or_default(),
            record
                .routing
                .as_ref()
                .map(|routing| routing.requested_model.clone())
                .unwrap_or_default(),
            record
                .routing
                .as_ref()
                .map(|routing| routing.selected_model.clone())
                .unwrap_or_default(),
            record
                .routing
                .as_ref()
                .map(|routing| routing.tier.clone())
                .unwrap_or_default(),
            record
                .routing
                .as_ref()
                .map(|routing| routing.task_type.clone())
                .unwrap_or_default(),
            record
                .routing
                .as_ref()
                .map(|routing| routing.reason_code.clone())
                .unwrap_or_default(),
            record
                .routing
                .as_ref()
                .and_then(|routing| routing.fallback_reason.clone())
                .unwrap_or_default(),
            optional_i32(record.status_code),
            record.streamed.to_string(),
            record.latency_ms.to_string(),
            optional_i64(record.first_response_ms),
            optional_f64(record.output_tokens_per_second),
            optional_i64(record.input_tokens),
            optional_i64(record.output_tokens),
            optional_i64(record.total_tokens),
            optional_i64(record.cache_in_tokens),
            optional_i64(record.cache_create_in_tokens),
            optional_i64(record.cache_create_5m_in_tokens),
            optional_i64(record.cache_create_1h_in_tokens),
            optional_i64(record.reason_out_tokens),
            optional_i64(record.audio_in_tokens),
            optional_i64(record.audio_out_tokens),
            record.billing_meter,
            record.billable_units.to_string(),
            optional_i64(record.cost_micros),
            record.billing_status,
            record.error_summary.unwrap_or_default(),
        ]
    }));

    rows
}

fn usage_export_filename(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> String {
    let start = start.map_or_else(
        || "all".to_string(),
        |value| value.format("%Y%m%d%H%M%S").to_string(),
    );
    let end = end.map_or_else(
        || "all".to_string(),
        |value| value.format("%Y%m%d%H%M%S").to_string(),
    );
    format!("usage-details-{start}-{end}.csv")
}

fn optional_id(value: Option<DbId>) -> String {
    value.map(|id| id.to_string()).unwrap_or_default()
}

fn optional_i32(value: Option<i32>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn optional_i64(value: Option<i64>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn optional_f64(value: Option<f64>) -> String {
    value.map(|value| format!("{value:.2}")).unwrap_or_default()
}

fn csv_response(filename: &str, rows: Vec<Vec<String>>) -> AppResult<Response> {
    let mut body = String::from('\u{FEFF}');
    body.push_str(
        &rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|value| escape_csv(value))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect::<Vec<_>>()
            .join("\n"),
    );
    body.push('\n');

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|_| AppError::BadRequest("invalid export filename".to_string()))?,
    );
    Ok((headers, Body::from(body)).into_response())
}

fn escape_csv(value: &str) -> String {
    if value.contains('"') || value.contains(',') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}
