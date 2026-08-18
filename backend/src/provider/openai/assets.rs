use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    core::net::validate_public_url,
    error::{reqwest_status, AppError, AppResult},
    project::models::ResolvedProjectModel,
    provider::adapters::{
        adapter_for_endpoint, AssetCreateRequest, AssetType, NormalizedAsset, ProviderAdapter,
    },
    relay::selector::{SelectedUpstream, UpstreamProtocol},
    relay::{forward_prepared_openai, RelayBody},
    AppState,
};

const ASSET_URI_PREFIX: &str = "asset://";
const UPSTREAM_ASSET_URI_PREFIX: &str = "assetId://";

#[derive(Debug, Deserialize)]
struct CreateAssetBody {
    model: String,
    #[serde(rename = "type")]
    asset_type: String,
    url: String,
    name: Option<String>,
}

#[derive(Debug, Serialize)]
struct AssetResponse {
    id: String,
    #[serde(rename = "type")]
    asset_type: String,
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct AssetRecord {
    public_id: String,
    project_id: i64,
    model: String,
    asset_type: AssetType,
    source_url: String,
    name: Option<String>,
    channel_endpoint_id: Option<i64>,
    channel_key_id: Option<i64>,
    credential_id: Option<i64>,
    upstream_asset_id: String,
    status: String,
    error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedVideoAssets {
    pub(crate) upstream: SelectedUpstream,
    pub(crate) body: Bytes,
}

pub(crate) async fn openai_assets_create(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    require_json(&headers)?;
    let request: CreateAssetBody = serde_json::from_slice(&body)
        .map_err(|err| AppError::BadRequest(format!("invalid asset request: {err}")))?;
    let asset_type = AssetType::parse(&request.asset_type)
        .ok_or_else(|| AppError::BadRequest("type must be image, video, or audio".into()))?;
    validate_public_url(&request.url)?;
    validate_name(request.name.as_deref())?;

    let resolved = crate::project::models::resolve_project_model(
        &state.db.pool,
        auth.project_id,
        &request.model,
    )
    .await?;
    let upstream = state
        .selector
        .select_matching_endpoint(
            &state.db.pool,
            &state.secrets,
            UpstreamProtocol::Openai,
            &resolved.target_model,
            resolved.target_channel_id,
            |channel| {
                adapter_for_endpoint(
                    &channel.provider,
                    &channel.base_url,
                    channel.adapter_hint.as_deref(),
                )
                .supports_assets(&resolved.target_model)
            },
        )
        .await?;
    let adapter = adapter_for_endpoint(
        &upstream.provider,
        &upstream.base_url,
        upstream.adapter_hint.as_deref(),
    );
    let prepared = adapter.prepare_asset_create_request(
        &upstream,
        &resolved.target_model,
        &AssetCreateRequest {
            asset_type,
            url: request.url.clone(),
            name: request.name.clone(),
        },
    )?;
    let response = forward_prepared_openai(
        &state,
        &upstream,
        UpstreamProtocol::Openai,
        &HeaderMap::new(),
        prepared,
    )
    .await?;
    let status = reqwest_status(response.status());
    let response_body = response.bytes().await?;
    if !status.is_success() {
        return Err(AppError::UpstreamUnavailable(format!(
            "asset upload upstream returned {}",
            status.as_u16()
        )));
    }
    let normalized = adapter.normalize_asset_response(response_body)?;
    if normalized.asset_type != asset_type {
        return Err(AppError::UpstreamUnavailable(
            "asset upload upstream returned a mismatched asset type".into(),
        ));
    }
    let public_id = format!("asset_{}", Uuid::new_v4().simple());
    let record = insert_asset(
        &state.db.pool,
        &auth,
        &resolved,
        &upstream,
        &public_id,
        &request,
        normalized,
    )
    .await?;
    Ok((StatusCode::CREATED, JsonAsset(record)).into_response())
}

pub(crate) async fn openai_asset_detail(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    Path(asset_id): Path<String>,
) -> AppResult<Response> {
    let record = fetch_asset(&state.db.pool, auth.project_id, &asset_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let record = refresh_asset(&state, record).await?;
    Ok(JsonAsset(record).into_response())
}

pub(crate) async fn resolve_video_asset_request(
    state: &AppState,
    auth: &UserAuth,
    target_model: &str,
    body: Bytes,
) -> AppResult<Option<ResolvedVideoAssets>> {
    let mut value: Value = serde_json::from_slice(&body).map_err(|err| {
        AppError::BadRequest(format!(
            "asset:// references require a JSON video request: {err}"
        ))
    })?;
    let refs = collect_asset_refs(&value)?;
    if refs.is_empty() {
        return Ok(None);
    }
    let mut records = Vec::with_capacity(refs.len());
    let mut by_id = HashMap::new();
    for (asset_id, asset_type) in refs {
        let record = fetch_asset(&state.db.pool, auth.project_id, &asset_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if record.asset_type != asset_type {
            return Err(AppError::Conflict(format!(
                "asset {asset_id} is a {} asset, but the request uses it as {}",
                record.asset_type.as_str(),
                asset_type.as_str()
            )));
        }
        let record = refresh_asset(state, record).await?;
        if record.status != "active" {
            return Err(AppError::Conflict(format!(
                "asset {asset_id} is not active (status: {})",
                record.status
            )));
        }
        by_id.insert(asset_id, record.clone());
        records.push(record);
    }
    let first = records.first().expect("asset refs are non-empty");
    if records.iter().any(|record| {
        record.channel_endpoint_id != first.channel_endpoint_id
            || record.channel_key_id != first.channel_key_id
            || record.credential_id != first.credential_id
    }) {
        return Err(AppError::Conflict(
            "asset references must use the same upstream endpoint and key".into(),
        ));
    }
    let upstream = state
        .selector
        .select_exact_endpoint(
            &state.db.pool,
            &state.secrets,
            UpstreamProtocol::Openai,
            target_model,
            first.channel_endpoint_id.ok_or_else(|| {
                AppError::UpstreamUnavailable("asset endpoint binding is missing".into())
            })?,
            first.channel_key_id,
            first.credential_id,
        )
        .await?;
    let adapter = adapter_for_endpoint(
        &upstream.provider,
        &upstream.base_url,
        upstream.adapter_hint.as_deref(),
    );
    if !adapter.supports_assets(target_model) {
        return Err(AppError::Conflict(
            "the bound upstream does not support assets for this model".into(),
        ));
    }
    rewrite_asset_refs(&mut value, &by_id, adapter)?;
    Ok(Some(ResolvedVideoAssets {
        upstream,
        body: Bytes::from(serde_json::to_vec(&value)?),
    }))
}

struct JsonAsset(AssetRecord);

impl IntoResponse for JsonAsset {
    fn into_response(self) -> Response {
        axum::Json(asset_response(self.0)).into_response()
    }
}

fn asset_response(record: AssetRecord) -> AssetResponse {
    AssetResponse {
        id: record.public_id,
        asset_type: record.asset_type.as_str().to_string(),
        url: record.source_url,
        name: record.name,
        status: record.status,
        error: record.error_message,
    }
}

async fn insert_asset(
    pool: &PgPool,
    auth: &UserAuth,
    resolved: &ResolvedProjectModel,
    upstream: &SelectedUpstream,
    public_id: &str,
    request: &CreateAssetBody,
    normalized: NormalizedAsset,
) -> AppResult<AssetRecord> {
    let row = sqlx::query(
        r#"
        INSERT INTO user_asset (
            public_id, project_id, model, asset_type, source_url, name, channel_endpoint_id,
            channel_key_id, credential_id,
            upstream_asset_id, status, error_message
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        RETURNING *
        "#,
    )
    .bind(public_id)
    .bind(auth.project_id)
    .bind(&resolved.target_model)
    .bind(normalized.asset_type.as_str())
    .bind(&request.url)
    .bind(request.name.as_deref().or(normalized.name.as_deref()))
    .bind(upstream.channel_endpoint_id)
    .bind(upstream.channel_key_id)
    .bind(upstream.credential_id)
    .bind(&normalized.upstream_asset_id)
    .bind(&normalized.status)
    .bind(&normalized.error_message)
    .fetch_one(pool)
    .await?;
    asset_from_row(&row)
}

async fn refresh_asset(state: &AppState, mut record: AssetRecord) -> AppResult<AssetRecord> {
    let upstream = state
        .selector
        .select_exact_endpoint(
            &state.db.pool,
            &state.secrets,
            UpstreamProtocol::Openai,
            &record.model,
            record.channel_endpoint_id.ok_or_else(|| {
                AppError::UpstreamUnavailable("asset endpoint binding is missing".into())
            })?,
            record.channel_key_id,
            record.credential_id,
        )
        .await?;
    let adapter = adapter_for_endpoint(
        &upstream.provider,
        &upstream.base_url,
        upstream.adapter_hint.as_deref(),
    );
    let prepared = adapter.prepare_asset_detail_request(
        &upstream,
        &record.model,
        &record.upstream_asset_id,
    )?;
    let response = forward_prepared_openai(
        state,
        &upstream,
        UpstreamProtocol::Openai,
        &HeaderMap::new(),
        prepared,
    )
    .await?;
    let status = reqwest_status(response.status());
    let response_body = response.bytes().await?;
    if !status.is_success() {
        return Err(AppError::UpstreamUnavailable(format!(
            "asset detail upstream returned {}",
            status.as_u16()
        )));
    }
    let normalized = adapter.normalize_asset_response(response_body)?;
    if normalized.asset_type != record.asset_type {
        return Err(AppError::UpstreamUnavailable(
            "asset detail upstream returned a mismatched asset type".into(),
        ));
    }
    sqlx::query(
        "UPDATE user_asset SET upstream_asset_id = $2, status = $3, error_message = $4, updated_at = now() WHERE project_id = $1 AND public_id = $5",
    )
    .bind(record.project_id)
    .bind(&normalized.upstream_asset_id)
    .bind(&normalized.status)
    .bind(&normalized.error_message)
    .bind(&record.public_id)
    .execute(&state.db.pool)
    .await?;
    record.upstream_asset_id = normalized.upstream_asset_id;
    record.status = normalized.status;
    record.error_message = normalized.error_message;
    Ok(record)
}

async fn fetch_asset(
    pool: &PgPool,
    project_id: i64,
    public_id: &str,
) -> AppResult<Option<AssetRecord>> {
    let row = sqlx::query("SELECT * FROM user_asset WHERE project_id = $1 AND public_id = $2")
        .bind(project_id)
        .bind(public_id)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(asset_from_row).transpose()
}

fn asset_from_row(row: &sqlx::postgres::PgRow) -> AppResult<AssetRecord> {
    Ok(AssetRecord {
        public_id: row.try_get("public_id")?,
        project_id: row.try_get("project_id")?,
        model: row.try_get("model")?,
        asset_type: AssetType::parse(row.try_get::<String, _>("asset_type")?.as_str())
            .ok_or_else(|| AppError::BadRequest("stored asset has invalid type".into()))?,
        source_url: row.try_get("source_url")?,
        name: row.try_get("name")?,
        channel_endpoint_id: row.try_get("channel_endpoint_id")?,
        channel_key_id: row.try_get("channel_key_id")?,
        credential_id: row.try_get("credential_id")?,
        upstream_asset_id: row.try_get("upstream_asset_id")?,
        status: row.try_get("status")?,
        error_message: row.try_get("error_message")?,
    })
}

fn require_json(headers: &HeaderMap) -> AppResult<()> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/json");
    if content_type
        .to_ascii_lowercase()
        .starts_with("application/json")
    {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "assets requests require application/json".into(),
        ))
    }
}

// validate_public_url 已迁移到 crate::core::net，此处直接 use

fn validate_name(name: Option<&str>) -> AppResult<()> {
    if name.is_some_and(|name| name.chars().count() > 50) {
        return Err(AppError::BadRequest(
            "name must contain at most 50 Unicode characters".into(),
        ));
    }
    Ok(())
}

fn collect_asset_refs(value: &Value) -> AppResult<Vec<(String, AssetType)>> {
    let mut refs = Vec::new();
    if let Some(url) = value.get("input_reference").and_then(input_reference_url) {
        if url.starts_with(UPSTREAM_ASSET_URI_PREFIX) {
            return Err(AppError::BadRequest(
                "provider asset URIs are not accepted; use asset://asset_*".into(),
            ));
        }
        if let Some(asset_id) = url.strip_prefix(ASSET_URI_PREFIX) {
            if asset_id.is_empty() || asset_id.contains('/') || asset_id.contains(':') {
                return Err(AppError::BadRequest("invalid asset URI".into()));
            }
            refs.push((asset_id.to_string(), AssetType::Image));
        }
    }
    let Some(content) = value.get("content").and_then(Value::as_array) else {
        return Ok(refs);
    };
    for item in content {
        let Some(item_type) = item.get("type").and_then(Value::as_str) else {
            continue;
        };
        let (field, asset_type) = match item_type {
            "image_url" => ("image_url", AssetType::Image),
            "video_url" => ("video_url", AssetType::Video),
            "audio_url" => ("audio_url", AssetType::Audio),
            _ => continue,
        };
        let Some(url) = item
            .get(field)
            .and_then(Value::as_object)
            .and_then(|object| object.get("url"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        if url.starts_with(UPSTREAM_ASSET_URI_PREFIX) {
            return Err(AppError::BadRequest(
                "provider asset URIs are not accepted; use asset://asset_*".into(),
            ));
        }
        if let Some(asset_id) = url.strip_prefix(ASSET_URI_PREFIX) {
            if asset_id.is_empty() || asset_id.contains('/') || asset_id.contains(':') {
                return Err(AppError::BadRequest("invalid asset URI".into()));
            }
            refs.push((asset_id.to_string(), asset_type));
        }
    }
    Ok(refs)
}

fn rewrite_asset_refs(
    value: &mut Value,
    records: &HashMap<String, AssetRecord>,
    adapter: &dyn ProviderAdapter,
) -> AppResult<()> {
    if let Some(url) = value
        .get_mut("input_reference")
        .and_then(input_reference_url_mut)
    {
        if let Some(asset_id) = url
            .as_str()
            .and_then(|value| value.strip_prefix(ASSET_URI_PREFIX))
        {
            let record = records.get(asset_id).ok_or(AppError::NotFound)?;
            *url = Value::String(
                adapter.format_asset_reference(AssetType::Image, &record.upstream_asset_id)?,
            );
        }
    }
    let Some(content) = value.get_mut("content").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for item in content {
        let Some(item_type) = item.get("type").and_then(Value::as_str) else {
            continue;
        };
        let (field, asset_type) = match item_type {
            "image_url" => ("image_url", AssetType::Image),
            "video_url" => ("video_url", AssetType::Video),
            "audio_url" => ("audio_url", AssetType::Audio),
            _ => continue,
        };
        let Some(url) = item
            .get_mut(field)
            .and_then(Value::as_object_mut)
            .and_then(|object| object.get_mut("url"))
        else {
            continue;
        };
        let Some(asset_id) = url
            .as_str()
            .and_then(|value| value.strip_prefix(ASSET_URI_PREFIX))
        else {
            continue;
        };
        let record = records.get(asset_id).ok_or_else(|| AppError::NotFound)?;
        *url =
            Value::String(adapter.format_asset_reference(asset_type, &record.upstream_asset_id)?);
    }
    Ok(())
}

fn input_reference_url(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.as_object()?.get("image_url")?.as_str())
}

fn input_reference_url_mut(value: &mut Value) -> Option<&mut Value> {
    if value.is_string() {
        return Some(value);
    }
    value.as_object_mut()?.get_mut("image_url")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_request_uses_flat_url_shape() {
        let request: CreateAssetBody = serde_json::from_value(serde_json::json!({
            "model": "sd_2.0_discount",
            "type": "image",
            "url": "https://example.com/person.png",
            "name": "reference"
        }))
        .unwrap();

        assert_eq!(request.model, "sd_2.0_discount");
        assert_eq!(request.asset_type, "image");
        assert_eq!(request.url, "https://example.com/person.png");
        assert_eq!(request.name.as_deref(), Some("reference"));
    }

    #[test]
    fn create_request_rejects_legacy_source_shape() {
        let request = serde_json::from_value::<CreateAssetBody>(serde_json::json!({
            "model": "sd_2.0_discount",
            "type": "image",
            "source": {
                "type": "url",
                "url": "https://example.com/person.png"
            }
        }));

        assert!(request.is_err());
    }

    #[test]
    fn response_omits_empty_optional_fields() {
        let response = AssetResponse {
            id: "asset_test".into(),
            asset_type: "image".into(),
            url: "https://example.com/person.png".into(),
            name: None,
            status: "processing".into(),
            error: None,
        };
        let value = serde_json::to_value(response).unwrap();

        assert_eq!(
            value,
            serde_json::json!({
                "id": "asset_test",
                "type": "image",
                "url": "https://example.com/person.png",
                "status": "processing"
            })
        );
    }

    #[test]
    fn response_includes_present_optional_fields() {
        let response = AssetResponse {
            id: "asset_test".into(),
            asset_type: "video".into(),
            url: "https://example.com/video.mp4".into(),
            name: Some("reference".into()),
            status: "failed".into(),
            error: Some("download failed".into()),
        };
        let value = serde_json::to_value(response).unwrap();

        assert_eq!(value["name"], "reference");
        assert_eq!(value["error"], "download failed");
    }

    #[test]
    fn collects_asset_from_json_input_reference_object() {
        let value = serde_json::json!({
            "input_reference": {
                "image_url": "asset://asset_reference"
            }
        });

        assert_eq!(
            collect_asset_refs(&value).unwrap(),
            vec![("asset_reference".to_string(), AssetType::Image)]
        );
    }
}
