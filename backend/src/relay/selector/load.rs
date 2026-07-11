use std::{
    collections::HashMap,
    sync::{atomic::AtomicUsize, Arc},
    time::Instant,
};

use chrono::{DateTime, Utc};
use sqlx::{AssertSqlSafe, PgPool, Row};

use crate::{
    admin::{channel::KeySelectionMode, credentials::openai_runtime_credential},
    billing::BILLABLE_PRICE_CONDITION_CP,
    error::{AppError, AppResult},
    id::DbId,
    secrets::SecretStore,
};

use super::{
    CachedRuntimeSecret, ChannelCandidate, KeyCandidate, ModelBlockKey, PlanModel, RouteIndex,
    RoutingCache, RuntimeSecretCache, UpstreamProtocol, WildcardRouteIndex,
    RUNTIME_SECRET_CACHE_MAX_ENTRIES,
};

pub(super) async fn load_routing_cache(pool: &PgPool) -> AppResult<RoutingCache> {
    let (channels, keys, model_blocks) = tokio::try_join!(
        fetch_channel_candidates(pool),
        fetch_key_candidates(pool),
        fetch_model_blocks(pool)
    )?;
    let (route_index, wildcard_index) = build_route_indexes(&channels);
    Ok(RoutingCache {
        loaded_at: Some(Instant::now()),
        channels,
        keys,
        model_blocks,
        route_index,
        wildcard_index,
    })
}

pub(super) fn build_route_indexes(
    channels: &[ChannelCandidate],
) -> (RouteIndex, WildcardRouteIndex) {
    let mut route_index: RouteIndex = HashMap::new();
    let mut wildcard_index: WildcardRouteIndex = HashMap::new();

    for (index, channel) in channels.iter().enumerate() {
        if channel.models.is_empty() {
            wildcard_index
                .entry(channel.protocol)
                .or_default()
                .push(index);
            continue;
        }
        for model in &channel.models {
            route_index
                .entry(channel.protocol)
                .or_default()
                .entry(model.clone())
                .or_default()
                .push(index);
        }
    }

    (route_index, wildcard_index)
}

async fn fetch_channel_candidates(pool: &PgPool) -> AppResult<Vec<ChannelCandidate>> {
    let rows = sqlx::query(AssertSqlSafe(format!(
        "SELECT c.id, ce.id AS endpoint_id, ce.protocol, c.provider, c.name,
                ce.base_url,
                COALESCE(
                    array_agg(cm.model ORDER BY cm.model ASC)
                        FILTER (WHERE cm.model IS NOT NULL),
                    ARRAY[]::TEXT[]
                ) AS models,
                c.priority, c.weight, c.key_selection_mode, ce.cooldown_until, c.use_credentials
         FROM channel c
         JOIN provider p ON p.code = c.provider
         JOIN channel_endpoint ce ON ce.channel_id = c.id
         LEFT JOIN channel_model cm
           ON cm.channel_id = c.id
          AND EXISTS (
              SELECT 1
              FROM unnest(ce.models) AS endpoint_model(model)
              WHERE btrim(endpoint_model.model) = cm.model
          )
          AND cm.enabled = TRUE
          AND cm.status = 'available'
          AND (
              cm.runtime_status = 'normal'
              OR (cm.runtime_status = 'cooldown' AND cm.cooldown_until <= now())
          )
          AND EXISTS (
              SELECT 1
              FROM channel_price cp
              WHERE cp.channel_id = c.id
                AND cp.model = cm.model
                AND cp.enabled = TRUE
                AND {BILLABLE_PRICE_CONDITION_CP}
          )
         WHERE p.enabled = TRUE
           AND c.enabled = TRUE
           AND ce.enabled = TRUE
           AND ce.healthy = TRUE
           AND (
               cm.id IS NOT NULL
               OR NOT EXISTS (
                   SELECT 1
                   FROM unnest(ce.models) AS endpoint_model(model)
                   WHERE btrim(endpoint_model.model) <> ''
               )
           )
           AND (
               (
                   c.use_credentials = FALSE
                   AND EXISTS (
                       SELECT 1 FROM channel_key ck
                       WHERE ck.channel_id = c.id
                         AND ck.enabled = TRUE
                         AND ck.healthy = TRUE
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
         GROUP BY c.id, ce.id
         ORDER BY c.priority DESC, c.created_at ASC"
    )))
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(channel_candidate_from_row)
        .collect::<AppResult<_>>()
}

async fn fetch_key_candidates(pool: &PgPool) -> AppResult<HashMap<DbId, Vec<KeyCandidate>>> {
    let plan_models = fetch_provider_plan_models(pool).await?;
    let rows = sqlx::query(
        "SELECT ck.id, ck.channel_id, NULL::BIGINT AS credential_id, ck.secret_ciphertext,
                ck.cooldown_until, NULL::TEXT AS provider, NULL::TEXT AS plan_type, ck.created_at
         FROM channel_key ck
         JOIN channel c ON c.id = ck.channel_id
         WHERE ck.enabled = TRUE
           AND ck.healthy = TRUE
           AND c.use_credentials = FALSE
         UNION ALL
         SELECT cr.id, c.id AS channel_id, cr.id AS credential_id,
                cr.content_ciphertext AS secret_ciphertext, NULL::TIMESTAMPTZ AS cooldown_until,
                cr.provider, cr.plan_type, cr.created_at
         FROM credential cr
         JOIN channel c ON c.provider = cr.provider
         WHERE c.enabled = TRUE
           AND c.use_credentials = TRUE
           AND cr.enabled = TRUE
         ORDER BY channel_id ASC, created_at ASC",
    )
    .fetch_all(pool)
    .await?;

    let mut keys = HashMap::new();
    for row in rows {
        let provider: Option<String> = row.try_get("provider")?;
        let plan_type: Option<String> = row.try_get("plan_type")?;
        let key_plan_models = provider
            .as_ref()
            .zip(plan_type.as_ref())
            .and_then(|(provider, plan_type)| {
                plan_models
                    .get(&(provider.clone(), plan_type.clone()))
                    .cloned()
            })
            .unwrap_or_default();
        let key = KeyCandidate {
            id: row.try_get("id")?,
            channel_id: row.try_get("channel_id")?,
            credential_id: row.try_get("credential_id")?,
            secret_ciphertext: row.try_get("secret_ciphertext")?,
            cooldown_until: row.try_get("cooldown_until")?,
            plan_type,
            plan_models: key_plan_models,
        };
        keys.entry(key.channel_id)
            .or_insert_with(Vec::new)
            .push(key);
    }
    Ok(keys)
}

async fn fetch_provider_plan_models(
    pool: &PgPool,
) -> AppResult<HashMap<(String, String), Vec<PlanModel>>> {
    let rows = sqlx::query(
        "SELECT provider, protocol, plan_type, model
         FROM provider_plan
         WHERE enabled = TRUE
         ORDER BY provider ASC, plan_type ASC, model ASC",
    )
    .fetch_all(pool)
    .await?;

    let mut plans: HashMap<(String, String), Vec<PlanModel>> = HashMap::new();
    for row in rows {
        let provider: String = row.try_get("provider")?;
        let plan_type: String = row.try_get("plan_type")?;
        let protocol: String = row.try_get("protocol")?;
        let protocol = match protocol.as_str() {
            "openai" => UpstreamProtocol::Openai,
            "openai_oauth" => UpstreamProtocol::OpenAiOauth,
            "anthropic" => UpstreamProtocol::Anthropic,
            other => return Err(AppError::BadRequest(format!("invalid protocol: {other}"))),
        };
        plans
            .entry((provider, plan_type))
            .or_default()
            .push(PlanModel {
                protocol,
                model: row.try_get("model")?,
            });
    }
    Ok(plans)
}

async fn fetch_model_blocks(pool: &PgPool) -> AppResult<HashMap<ModelBlockKey, DateTime<Utc>>> {
    let rows = sqlx::query(
        "SELECT ce.protocol, cm.channel_endpoint_id, NULL::BIGINT AS channel_key_id,
                cm.credential_id, cm.model, cm.unavailable_until
         FROM credential_model cm
         JOIN channel_endpoint ce ON ce.id = cm.channel_endpoint_id
         WHERE cm.status = 'unavailable'
           AND cm.unavailable_until > now()",
    )
    .fetch_all(pool)
    .await?;

    let mut blocks = HashMap::new();
    for row in rows {
        let protocol: String = row.try_get("protocol")?;
        let protocol = match protocol.as_str() {
            "openai" => UpstreamProtocol::Openai,
            "openai_oauth" => UpstreamProtocol::OpenAiOauth,
            "anthropic" => UpstreamProtocol::Anthropic,
            other => return Err(AppError::BadRequest(format!("invalid protocol: {other}"))),
        };
        let unavailable_until: DateTime<Utc> = row.try_get("unavailable_until")?;
        blocks.insert(
            ModelBlockKey {
                protocol,
                endpoint_id: row.try_get("channel_endpoint_id")?,
                channel_key_id: row.try_get("channel_key_id")?,
                credential_id: row.try_get("credential_id")?,
                model: row.try_get("model")?,
            },
            unavailable_until,
        );
    }
    Ok(blocks)
}

fn channel_candidate_from_row(row: &sqlx::postgres::PgRow) -> AppResult<ChannelCandidate> {
    let provider: String = row.try_get("provider")?;
    let protocol: String = row.try_get("protocol")?;
    let mode: String = row.try_get("key_selection_mode")?;
    Ok(ChannelCandidate {
        id: row.try_get("id")?,
        endpoint_id: row.try_get("endpoint_id")?,
        protocol: match protocol.as_str() {
            "openai" => UpstreamProtocol::Openai,
            "openai_oauth" => UpstreamProtocol::OpenAiOauth,
            "anthropic" => UpstreamProtocol::Anthropic,
            other => return Err(AppError::BadRequest(format!("invalid protocol: {other}"))),
        },
        provider,
        name: row.try_get("name")?,
        base_url: row.try_get("base_url")?,
        models: row.try_get("models")?,
        priority: row.try_get("priority")?,
        weight: row.try_get("weight")?,
        cooldown_until: row.try_get("cooldown_until")?,
        key_selection_mode: match mode.as_str() {
            "polling" => KeySelectionMode::Polling,
            "random" => KeySelectionMode::Random,
            other => {
                return Err(AppError::BadRequest(format!(
                    "invalid key selection mode: {other}"
                )))
            }
        },
        use_credentials: row.try_get("use_credentials")?,
        polling: Arc::new(AtomicUsize::new(0)),
    })
}

pub(super) fn credential_runtime_secret(
    cache: &RuntimeSecretCache,
    secrets: &SecretStore,
    credential_id: DbId,
    content_ciphertext: &str,
) -> AppResult<CachedRuntimeSecret> {
    if let Some(secret) = cache.get(credential_id, content_ciphertext) {
        return Ok(secret);
    }
    let value: serde_json::Value =
        serde_json::from_str(&secrets.plaintext(credential_id, content_ciphertext)?)?;
    let credential = openai_runtime_credential(&value)
        .ok_or_else(|| AppError::BadRequest("credential has no usable OpenAI token".to_string()))?;
    let runtime = CachedRuntimeSecret {
        ciphertext: content_ciphertext.to_string(),
        secret: credential.access_token,
        account_id: credential.account_id,
    };
    cache.insert(credential_id, runtime.clone());
    Ok(runtime)
}

pub(super) fn trim_runtime_secret_cache_for_insert(
    entries: &mut HashMap<DbId, CachedRuntimeSecret>,
    keep: DbId,
) {
    while entries.len() >= RUNTIME_SECRET_CACHE_MAX_ENTRIES && !entries.contains_key(&keep) {
        let Some(evict) = entries.keys().next().copied() else {
            break;
        };
        entries.remove(&evict);
    }
}
