use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, RwLock as StdRwLock,
    },
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use rand::RngExt;
use sqlx::{PgPool, Row};
use tokio::sync::{Mutex, RwLock};

use crate::{
    admin::{channel::KeySelectionMode, credentials::openai_runtime_credential},
    error::{AppError, AppResult},
    id::DbId,
    secrets::SecretStore,
};

const RUNTIME_SECRET_CACHE_MAX_ENTRIES: usize = 4096;

type RouteIndex = HashMap<(UpstreamProtocol, String), Vec<usize>>;
type WildcardRouteIndex = HashMap<UpstreamProtocol, Vec<usize>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpstreamProtocol {
    Openai,
    OpenAiOauth,
    Anthropic,
}

impl UpstreamProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::OpenAiOauth => "openai_oauth",
            Self::Anthropic => "anthropic",
        }
    }
}

#[derive(Clone)]
pub struct Selector {
    routing_cache: Arc<RwLock<Arc<RoutingCache>>>,
    reload_lock: Arc<Mutex<()>>,
    routing_cache_ttl: Duration,
    credential_runtime_secrets: RuntimeSecretCache,
    model_blocks: ModelBlockCache,
}

#[derive(Clone, Default)]
struct RuntimeSecretCache {
    entries: Arc<StdRwLock<HashMap<DbId, CachedRuntimeSecret>>>,
}

#[derive(Clone)]
struct CachedRuntimeSecret {
    ciphertext: String,
    secret: String,
    account_id: Option<String>,
}

#[derive(Clone, Default)]
struct ModelBlockCache {
    entries: Arc<StdRwLock<HashMap<ModelBlockKey, DateTime<Utc>>>>,
}

#[derive(Clone, Debug, Eq)]
struct ModelBlockKey {
    protocol: UpstreamProtocol,
    endpoint_id: DbId,
    channel_key_id: Option<DbId>,
    credential_id: Option<DbId>,
    model: String,
}

impl PartialEq for ModelBlockKey {
    fn eq(&self, other: &Self) -> bool {
        self.protocol == other.protocol
            && self.endpoint_id == other.endpoint_id
            && self.channel_key_id == other.channel_key_id
            && self.credential_id == other.credential_id
            && self.model == other.model
    }
}

impl Hash for ModelBlockKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.protocol.hash(state);
        self.endpoint_id.hash(state);
        self.channel_key_id.hash(state);
        self.credential_id.hash(state);
        self.model.hash(state);
    }
}

#[derive(Clone, Default)]
struct RoutingCache {
    loaded_at: Option<Instant>,
    channels: Vec<ChannelCandidate>,
    keys: HashMap<DbId, Vec<KeyCandidate>>,
    model_blocks: HashMap<ModelBlockKey, DateTime<Utc>>,
    route_index: RouteIndex,
    wildcard_index: WildcardRouteIndex,
}

#[derive(Debug, Clone)]
pub struct SelectedUpstream {
    pub channel_id: DbId,
    pub channel_endpoint_id: DbId,
    pub channel_key_id: Option<DbId>,
    pub credential_id: Option<DbId>,
    pub provider: String,
    pub channel_name: String,
    pub base_url: String,
    pub secret: String,
    pub account_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChannelCandidate {
    pub id: DbId,
    pub endpoint_id: DbId,
    pub protocol: UpstreamProtocol,
    pub provider: String,
    pub name: String,
    pub base_url: String,
    pub models: Vec<String>,
    pub priority: i32,
    pub weight: i32,
    pub key_selection_mode: KeySelectionMode,
    pub use_credentials: bool,
    pub cooldown_until: Option<DateTime<Utc>>,
    polling: Arc<AtomicUsize>,
}

#[derive(Debug, Clone)]
pub struct KeyCandidate {
    pub id: DbId,
    pub channel_id: DbId,
    pub credential_id: Option<DbId>,
    pub secret_ciphertext: String,
    pub cooldown_until: Option<DateTime<Utc>>,
    plan_type: Option<String>,
    plan_models: Vec<PlanModel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanModel {
    protocol: UpstreamProtocol,
    model: String,
}

struct ModelBlockLookup<'a> {
    persisted: &'a HashMap<ModelBlockKey, DateTime<Utc>>,
    local: &'a HashMap<ModelBlockKey, DateTime<Utc>>,
}

impl<'a> ModelBlockLookup<'a> {
    fn new(
        persisted: &'a HashMap<ModelBlockKey, DateTime<Utc>>,
        local: &'a HashMap<ModelBlockKey, DateTime<Utc>>,
    ) -> Self {
        Self { persisted, local }
    }

    fn contains_active(&self, key: &ModelBlockKey, now: DateTime<Utc>) -> bool {
        self.local
            .get(key)
            .or_else(|| self.persisted.get(key))
            .map(|blocked_until| *blocked_until > now)
            .unwrap_or(false)
    }
}

impl Selector {
    pub fn new() -> Self {
        Self::with_cache_ttl(Duration::from_secs(30))
    }

    pub fn with_cache_ttl(routing_cache_ttl: Duration) -> Self {
        Self {
            routing_cache: Arc::new(RwLock::new(Arc::new(RoutingCache::default()))),
            reload_lock: Arc::new(Mutex::new(())),
            routing_cache_ttl,
            credential_runtime_secrets: RuntimeSecretCache::default(),
            model_blocks: ModelBlockCache::default(),
        }
    }

    pub async fn invalidate(&self) {
        let mut cache = self.routing_cache.write().await;
        let mut next = (**cache).clone();
        next.loaded_at = None;
        *cache = Arc::new(next);
        self.credential_runtime_secrets.clear();
        self.model_blocks.clear_expired(Utc::now());
    }

    pub async fn select(
        &self,
        pool: &PgPool,
        secrets: &SecretStore,
        protocol: UpstreamProtocol,
        model: &str,
    ) -> AppResult<SelectedUpstream> {
        let snapshot = self.routing_snapshot(pool).await?;
        let now = Utc::now();
        let local_model_blocks = self.model_blocks.snapshot(now);
        let model_blocks = ModelBlockLookup::new(&snapshot.model_blocks, &local_model_blocks);
        let channel = choose_channel_for_request(&snapshot, protocol, model, now, &model_blocks)
            .ok_or_else(|| {
                AppError::UpstreamUnavailable(unavailable_channel_message(
                    &snapshot,
                    protocol,
                    model,
                    now,
                    &model_blocks,
                ))
            })?;
        let keys = snapshot
            .keys
            .get(&channel.id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let key = choose_key(channel, keys, model, now, &model_blocks).ok_or_else(|| {
            AppError::UpstreamUnavailable(format!("channel {} has no available key", channel.name))
        })?;

        let runtime = if let Some(credential_id) = key.credential_id {
            credential_runtime_secret(
                &self.credential_runtime_secrets,
                secrets,
                credential_id,
                &key.secret_ciphertext,
            )?
        } else {
            CachedRuntimeSecret {
                ciphertext: key.secret_ciphertext.clone(),
                secret: secrets.plaintext(key.id, &key.secret_ciphertext)?,
                account_id: None,
            }
        };

        Ok(SelectedUpstream {
            channel_id: channel.id,
            channel_endpoint_id: channel.endpoint_id,
            channel_key_id: (!channel.use_credentials).then_some(key.id),
            credential_id: key.credential_id,
            provider: channel.provider.clone(),
            channel_name: channel.name.clone(),
            base_url: channel.base_url.clone(),
            secret: runtime.secret,
            account_id: runtime.account_id,
        })
    }

    pub async fn mark_model_unavailable_local(
        &self,
        upstream: &SelectedUpstream,
        protocol: UpstreamProtocol,
        model: &str,
        blocked_until: DateTime<Utc>,
    ) {
        self.model_blocks.insert(
            ModelBlockKey {
                protocol,
                endpoint_id: upstream.channel_endpoint_id,
                channel_key_id: upstream.channel_key_id,
                credential_id: upstream.credential_id,
                model: model.to_string(),
            },
            blocked_until,
        );
    }

    pub async fn mark_credential_model_unavailable(
        &self,
        pool: &PgPool,
        upstream: &SelectedUpstream,
        protocol: UpstreamProtocol,
        model: &str,
        unavailable_until: DateTime<Utc>,
    ) -> AppResult<bool> {
        if !self
            .has_alternate_channel_for_model(pool, protocol, model)
            .await?
        {
            return Ok(false);
        }
        self.mark_model_unavailable_local(upstream, protocol, model, unavailable_until)
            .await;
        Ok(true)
    }

    pub async fn mark_key_failure_local(
        &self,
        channel_key_id: DbId,
        cooldown_until: DateTime<Utc>,
    ) {
        let mut guard = self.routing_cache.write().await;
        let mut cache = (**guard).clone();
        for keys in cache.keys.values_mut() {
            if let Some(key) = keys
                .iter_mut()
                .find(|key| key.credential_id.is_none() && key.id == channel_key_id)
            {
                key.cooldown_until = Some(cooldown_until);
                *guard = Arc::new(cache);
                return;
            }
        }
    }

    async fn routing_snapshot(&self, pool: &PgPool) -> AppResult<Arc<RoutingCache>> {
        {
            let cache = self.routing_cache.read().await;
            if cache.is_fresh(self.routing_cache_ttl) {
                return Ok(Arc::clone(&cache));
            }
        }

        let _reload = self.reload_lock.lock().await;
        {
            let cache = self.routing_cache.read().await;
            if cache.is_fresh(self.routing_cache_ttl) {
                return Ok(Arc::clone(&cache));
            }
        }

        let loaded = Arc::new(load_routing_cache(pool).await?);
        let mut cache = self.routing_cache.write().await;
        *cache = loaded;
        Ok(Arc::clone(&cache))
    }

    pub async fn mark_key_failure(
        &self,
        pool: &PgPool,
        channel_key_id: DbId,
        protocol: UpstreamProtocol,
        model: &str,
        error: &str,
        cooldown_until: DateTime<Utc>,
    ) -> AppResult<bool> {
        if !self
            .has_alternate_channel_for_model(pool, protocol, model)
            .await?
        {
            return Ok(false);
        }
        sqlx::query(
            "UPDATE channel_key
             SET cooldown_until = $2,
                 last_error = $3,
                 updated_at = now()
             WHERE id = $1",
        )
        .bind(channel_key_id)
        .bind(cooldown_until)
        .bind(error.chars().take(500).collect::<String>())
        .execute(pool)
        .await?;
        Ok(true)
    }

    pub async fn has_alternate_channel_for_model(
        &self,
        pool: &PgPool,
        protocol: UpstreamProtocol,
        model: &str,
    ) -> AppResult<bool> {
        let snapshot = self.routing_snapshot(pool).await?;
        Ok(matching_channel_count(&snapshot, protocol, model) > 1)
    }
}

impl RuntimeSecretCache {
    fn get(&self, credential_id: DbId, ciphertext: &str) -> Option<CachedRuntimeSecret> {
        self.entries
            .read()
            .expect("credential runtime secret cache poisoned")
            .get(&credential_id)
            .filter(|cached| cached.ciphertext == ciphertext)
            .cloned()
    }

    fn insert(&self, credential_id: DbId, runtime: CachedRuntimeSecret) {
        let mut entries = self
            .entries
            .write()
            .expect("credential runtime secret cache poisoned");
        trim_runtime_secret_cache_for_insert(&mut entries, credential_id);
        entries.insert(credential_id, runtime);
    }

    fn clear(&self) {
        self.entries
            .write()
            .expect("credential runtime secret cache poisoned")
            .clear();
    }
}

impl ModelBlockCache {
    fn insert(&self, key: ModelBlockKey, blocked_until: DateTime<Utc>) {
        let mut entries = self.entries.write().expect("model block cache poisoned");
        let now = Utc::now();
        entries.retain(|_, until| *until > now);
        entries.insert(key, blocked_until);
    }

    fn snapshot(&self, now: DateTime<Utc>) -> HashMap<ModelBlockKey, DateTime<Utc>> {
        let mut entries = self.entries.write().expect("model block cache poisoned");
        entries.retain(|_, until| *until > now);
        entries.clone()
    }

    fn clear_expired(&self, now: DateTime<Utc>) {
        self.entries
            .write()
            .expect("model block cache poisoned")
            .retain(|_, until| *until > now);
    }
}

impl RoutingCache {
    fn is_fresh(&self, ttl: Duration) -> bool {
        self.loaded_at
            .map(|loaded_at| loaded_at.elapsed() < ttl)
            .unwrap_or(false)
    }
}

async fn load_routing_cache(pool: &PgPool) -> AppResult<RoutingCache> {
    let channels = fetch_channel_candidates(pool).await?;
    let keys = fetch_key_candidates(pool).await?;
    let model_blocks = fetch_model_blocks(pool).await?;
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

fn build_route_indexes(channels: &[ChannelCandidate]) -> (RouteIndex, WildcardRouteIndex) {
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
                .entry((channel.protocol, model.clone()))
                .or_default()
                .push(index);
        }
    }

    (route_index, wildcard_index)
}

async fn fetch_channel_candidates(pool: &PgPool) -> AppResult<Vec<ChannelCandidate>> {
    let rows = sqlx::query(
        "SELECT c.id, ce.id AS endpoint_id, ce.protocol, c.provider, c.name,
                ce.base_url, ce.models, c.priority, c.weight, c.key_selection_mode,
                ce.cooldown_until, c.use_credentials
         FROM channel c
         JOIN provider p ON p.code = c.provider
         JOIN channel_endpoint ce ON ce.channel_id = c.id
         WHERE p.enabled = TRUE
           AND c.enabled = TRUE
           AND ce.enabled = TRUE
           AND ce.healthy = TRUE
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
         ORDER BY c.priority DESC, c.created_at ASC",
    )
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

fn credential_runtime_secret(
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

fn trim_runtime_secret_cache_for_insert(
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

fn ready_at(cooldown_until: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    cooldown_until.map(|value| value <= now).unwrap_or(true)
}

pub fn channel_matches_model(channel: &ChannelCandidate, model: &str) -> bool {
    channel.models.is_empty() || channel.models.iter().any(|item| item == model)
}

fn matching_channel_count(cache: &RoutingCache, protocol: UpstreamProtocol, model: &str) -> usize {
    cache
        .channels
        .iter()
        .filter(|channel| channel.protocol == protocol && channel_matches_model(channel, model))
        .count()
}

#[cfg(test)]
pub fn choose_channel(channels: &[ChannelCandidate]) -> Option<ChannelCandidate> {
    if channels.is_empty() {
        return None;
    }
    let highest_priority = channels.iter().map(|item| item.priority).max()?;
    let candidates: Vec<_> = channels
        .iter()
        .filter(|item| item.priority == highest_priority)
        .cloned()
        .collect();
    let total_weight: i32 = candidates.iter().map(|item| item.weight.max(1)).sum();
    let slot = rand::rng().random_range(0..total_weight);
    choose_channel_by_slot(&candidates, slot)
}

fn choose_channel_for_request<'a>(
    cache: &'a RoutingCache,
    protocol: UpstreamProtocol,
    model: &str,
    now: DateTime<Utc>,
    model_blocks: &ModelBlockLookup<'_>,
) -> Option<&'a ChannelCandidate> {
    let route_key = (protocol, model.to_string());
    let indexed = cache
        .route_index
        .get(&route_key)
        .into_iter()
        .chain(cache.wildcard_index.get(&protocol))
        .flat_map(|indexes| indexes.iter().copied())
        .filter_map(|index| cache.channels.get(index));

    let mut highest_priority = None;
    let mut total_weight = 0;
    let mut candidates = Vec::new();

    for channel in indexed {
        if !channel_is_available(cache, channel, protocol, model, now, model_blocks) {
            continue;
        }
        match highest_priority {
            None => {
                highest_priority = Some(channel.priority);
                total_weight = channel.weight.max(1);
                candidates.push(channel);
            }
            Some(priority) if channel.priority > priority => {
                highest_priority = Some(channel.priority);
                total_weight = channel.weight.max(1);
                candidates.clear();
                candidates.push(channel);
            }
            Some(priority) if channel.priority == priority => {
                total_weight += channel.weight.max(1);
                candidates.push(channel);
            }
            Some(_) => {}
        }
    }

    highest_priority?;
    let mut slot = rand::rng().random_range(0..total_weight);
    candidates.into_iter().find(|channel| {
        let weight = channel.weight.max(1);
        if slot < weight {
            true
        } else {
            slot -= weight;
            false
        }
    })
}

fn channel_is_available(
    cache: &RoutingCache,
    channel: &ChannelCandidate,
    protocol: UpstreamProtocol,
    model: &str,
    now: DateTime<Utc>,
    model_blocks: &ModelBlockLookup<'_>,
) -> bool {
    channel.protocol == protocol
        && channel_matches_model(channel, model)
        && ready_at(channel.cooldown_until, now)
        && cache
            .keys
            .get(&channel.id)
            .map(|keys| {
                keys.iter()
                    .any(|key| key_is_available(channel, key, model, now, model_blocks))
            })
            .unwrap_or(false)
}

fn unavailable_channel_message(
    cache: &RoutingCache,
    protocol: UpstreamProtocol,
    model: &str,
    now: DateTime<Utc>,
    model_blocks: &ModelBlockLookup<'_>,
) -> String {
    let protocol_name = protocol.as_str();
    let protocol_channels: Vec<_> = cache
        .channels
        .iter()
        .filter(|channel| channel.protocol == protocol)
        .collect();

    if protocol_channels.is_empty() {
        let other_protocol_matches: Vec<_> = cache
            .channels
            .iter()
            .filter(|channel| channel.protocol != protocol && channel_matches_model(channel, model))
            .take(3)
            .map(|channel| format!("{} ({})", channel.name, channel.protocol.as_str()))
            .collect();

        if other_protocol_matches.is_empty() {
            return format!(
                "no available {protocol_name} channel for model {model}; add an enabled healthy {protocol_name} channel with an enabled healthy key"
            );
        }

        return format!(
            "no available {protocol_name} channel for model {model}; matching channel(s) use another protocol: {}",
            other_protocol_matches.join(", ")
        );
    }

    let matching_model_channels: Vec<_> = protocol_channels
        .iter()
        .copied()
        .filter(|channel| channel_matches_model(channel, model))
        .collect();
    if matching_model_channels.is_empty() {
        return format!(
            "no available {protocol_name} channel for model {model}; configured {protocol_name} channels do not include this model"
        );
    }

    let ready_channels: Vec<_> = matching_model_channels
        .iter()
        .copied()
        .filter(|channel| ready_at(channel.cooldown_until, now))
        .collect();
    if ready_channels.is_empty() {
        return format!(
            "no available {protocol_name} channel for model {model}; matching channel(s) are cooling down"
        );
    }

    if ready_channels.iter().all(|channel| {
        cache
            .keys
            .get(&channel.id)
            .map(|keys| {
                keys.iter()
                    .all(|key| !key_is_available(channel, key, model, now, model_blocks))
            })
            .unwrap_or(true)
    }) {
        return format!(
            "no available {protocol_name} channel for model {model}; matching channel(s) have no enabled healthy key ready to use"
        );
    }

    format!("no available {protocol_name} channel for model {model}")
}

fn choose_key<'a>(
    channel: &ChannelCandidate,
    keys: &'a [KeyCandidate],
    model: &str,
    now: DateTime<Utc>,
    model_blocks: &ModelBlockLookup<'_>,
) -> Option<&'a KeyCandidate> {
    let ready_count = keys
        .iter()
        .filter(|key| key_is_available(channel, key, model, now, model_blocks))
        .count();
    if ready_count == 0 {
        return None;
    }
    let slot = match channel.key_selection_mode {
        KeySelectionMode::Random => rand::rng().random_range(0..ready_count),
        KeySelectionMode::Polling => channel.polling.fetch_add(1, Ordering::Relaxed) % ready_count,
    };
    keys.iter()
        .filter(|key| key_is_available(channel, key, model, now, model_blocks))
        .nth(slot)
}

fn key_is_ready(key: &KeyCandidate, now: DateTime<Utc>) -> bool {
    ready_at(key.cooldown_until, now)
}

fn key_is_available(
    channel: &ChannelCandidate,
    key: &KeyCandidate,
    model: &str,
    now: DateTime<Utc>,
    model_blocks: &ModelBlockLookup<'_>,
) -> bool {
    key_is_ready(key, now)
        && key_plan_allows_model(channel, key, model)
        && !model_is_blocked(channel, key, model, now, model_blocks)
}

fn key_plan_allows_model(channel: &ChannelCandidate, key: &KeyCandidate, model: &str) -> bool {
    if key.credential_id.is_none() || key.plan_type.is_none() || key.plan_models.is_empty() {
        return true;
    }
    key.plan_models
        .iter()
        .any(|item| item.protocol == channel.protocol && item.model == model)
}

fn model_is_blocked(
    channel: &ChannelCandidate,
    key: &KeyCandidate,
    model: &str,
    now: DateTime<Utc>,
    model_blocks: &ModelBlockLookup<'_>,
) -> bool {
    let block_key = ModelBlockKey {
        protocol: channel.protocol,
        endpoint_id: channel.endpoint_id,
        channel_key_id: (!channel.use_credentials).then_some(key.id),
        credential_id: key.credential_id,
        model: model.to_string(),
    };
    model_blocks.contains_active(&block_key, now)
}

#[cfg(test)]
pub fn choose_channel_by_slot(
    channels: &[ChannelCandidate],
    mut slot: i32,
) -> Option<ChannelCandidate> {
    for channel in channels {
        let weight = channel.weight.max(1);
        if slot < weight {
            return Some(channel.clone());
        }
        slot -= weight;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::LazyLock;

    static EMPTY_BLOCKS: LazyLock<HashMap<ModelBlockKey, DateTime<Utc>>> =
        LazyLock::new(HashMap::new);

    fn empty_block_lookup() -> ModelBlockLookup<'static> {
        ModelBlockLookup::new(&EMPTY_BLOCKS, &EMPTY_BLOCKS)
    }

    fn candidate(name: &str, priority: i32, weight: i32, models: Vec<&str>) -> ChannelCandidate {
        ChannelCandidate {
            id: 1,
            endpoint_id: 10,
            protocol: UpstreamProtocol::Openai,
            provider: "openai".to_string(),
            name: name.to_string(),
            base_url: "https://example.com".to_string(),
            models: models.into_iter().map(str::to_string).collect(),
            priority,
            weight,
            key_selection_mode: KeySelectionMode::Polling,
            use_credentials: false,
            cooldown_until: None,
            polling: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[test]
    fn model_match_accepts_empty_model_list() {
        let channel = candidate("any", 0, 1, vec![]);
        assert!(channel_matches_model(&channel, "gpt-4.1"));
    }

    #[test]
    fn model_match_requires_exact_listed_model() {
        let channel = candidate("strict", 0, 1, vec!["gpt-4.1"]);
        assert!(channel_matches_model(&channel, "gpt-4.1"));
        assert!(!channel_matches_model(&channel, "gpt-4o-mini"));
    }

    #[test]
    fn choose_channel_uses_highest_priority() {
        let low = candidate("low", 0, 100, vec![]);
        let high = candidate("high", 2, 1, vec![]);
        let selected = choose_channel_by_slot(std::slice::from_ref(&high), 0).unwrap();
        assert_eq!(selected.name, "high");
        assert_eq!(choose_channel(&[low, high]).unwrap().priority, 2);
    }

    #[test]
    fn choose_channel_by_slot_respects_weight_ranges() {
        let a = candidate("a", 1, 2, vec![]);
        let b = candidate("b", 1, 3, vec![]);
        assert_eq!(
            choose_channel_by_slot(&[a.clone(), b.clone()], 0)
                .unwrap()
                .name,
            "a"
        );
        assert_eq!(
            choose_channel_by_slot(&[a.clone(), b.clone()], 1)
                .unwrap()
                .name,
            "a"
        );
        assert_eq!(choose_channel_by_slot(&[a, b], 2).unwrap().name, "b");
    }

    #[test]
    fn matching_channel_count_counts_model_and_wildcard_channels() {
        let mut exact = candidate("exact", 0, 1, vec!["gpt-5.5"]);
        exact.id = 1;
        let mut wildcard = candidate("wildcard", 0, 1, vec![]);
        wildcard.id = 2;
        let mut other_model = candidate("other", 0, 1, vec!["gpt-4.1"]);
        other_model.id = 3;
        let mut other_protocol = candidate("anthropic", 0, 1, vec!["gpt-5.5"]);
        other_protocol.id = 4;
        other_protocol.protocol = UpstreamProtocol::Anthropic;
        let cache = RoutingCache {
            loaded_at: None,
            channels: vec![exact, wildcard, other_model, other_protocol],
            keys: HashMap::new(),
            model_blocks: HashMap::new(),
            route_index: HashMap::new(),
            wildcard_index: HashMap::new(),
        };

        assert_eq!(
            matching_channel_count(&cache, UpstreamProtocol::Openai, "gpt-5.5"),
            2
        );
        assert_eq!(
            matching_channel_count(&cache, UpstreamProtocol::Openai, "gpt-4.1"),
            2
        );
        assert_eq!(
            matching_channel_count(&cache, UpstreamProtocol::Anthropic, "gpt-5.5"),
            1
        );
    }

    #[test]
    fn unavailable_message_reports_wrong_protocol_match() {
        let mut channel = candidate("deepseek", 0, 1, vec!["claude-sonnet-4-5"]);
        channel.protocol = UpstreamProtocol::Openai;
        let cache = RoutingCache {
            loaded_at: None,
            channels: vec![channel],
            keys: HashMap::new(),
            model_blocks: HashMap::new(),
            route_index: HashMap::new(),
            wildcard_index: HashMap::new(),
        };

        let message = unavailable_channel_message(
            &cache,
            UpstreamProtocol::Anthropic,
            "claude-sonnet-4-5",
            Utc::now(),
            &empty_block_lookup(),
        );

        assert!(message.contains("matching channel(s) use another protocol"));
        assert!(message.contains("deepseek (openai)"));
    }

    #[test]
    fn unavailable_message_reports_model_mismatch() {
        let mut channel = candidate("anthropic", 0, 1, vec!["claude-3-5-sonnet-latest"]);
        channel.protocol = UpstreamProtocol::Anthropic;
        let cache = RoutingCache {
            loaded_at: None,
            channels: vec![channel],
            keys: HashMap::new(),
            model_blocks: HashMap::new(),
            route_index: HashMap::new(),
            wildcard_index: HashMap::new(),
        };

        let message = unavailable_channel_message(
            &cache,
            UpstreamProtocol::Anthropic,
            "claude-sonnet-4-5",
            Utc::now(),
            &empty_block_lookup(),
        );

        assert!(message.contains("configured anthropic channels do not include this model"));
    }

    #[tokio::test]
    async fn polling_key_selection_cycles() {
        let channel = candidate("poll", 0, 1, vec![]);
        let keys = vec![
            KeyCandidate {
                id: 1,
                channel_id: channel.id,
                credential_id: None,
                secret_ciphertext: "a".to_string(),
                cooldown_until: None,
                plan_type: None,
                plan_models: Vec::new(),
            },
            KeyCandidate {
                id: 2,
                channel_id: channel.id,
                credential_id: None,
                secret_ciphertext: "b".to_string(),
                cooldown_until: None,
                plan_type: None,
                plan_models: Vec::new(),
            },
        ];

        assert_eq!(
            choose_key(
                &channel,
                &keys,
                "gpt-4.1",
                Utc::now(),
                &empty_block_lookup()
            )
            .unwrap()
            .secret_ciphertext,
            "a"
        );
        assert_eq!(
            choose_key(
                &channel,
                &keys,
                "gpt-4.1",
                Utc::now(),
                &empty_block_lookup()
            )
            .unwrap()
            .secret_ciphertext,
            "b"
        );
        assert_eq!(
            choose_key(
                &channel,
                &keys,
                "gpt-4.1",
                Utc::now(),
                &empty_block_lookup()
            )
            .unwrap()
            .secret_ciphertext,
            "a"
        );
    }

    #[tokio::test]
    async fn random_key_selection_uses_available_keys_only() {
        let mut channel = candidate("random", 0, 1, vec![]);
        channel.key_selection_mode = KeySelectionMode::Random;
        let keys = vec![KeyCandidate {
            id: 1,
            channel_id: channel.id,
            credential_id: None,
            secret_ciphertext: "only-enabled".to_string(),
            cooldown_until: None,
            plan_type: None,
            plan_models: Vec::new(),
        }];

        assert_eq!(
            choose_key(
                &channel,
                &keys,
                "gpt-4.1",
                Utc::now(),
                &empty_block_lookup()
            )
            .unwrap()
            .secret_ciphertext,
            "only-enabled"
        );
    }

    #[tokio::test]
    async fn key_selection_skips_model_blocked_credential() {
        let mut channel = candidate("oauth", 0, 1, vec!["gpt-5.4"]);
        channel.protocol = UpstreamProtocol::OpenAiOauth;
        channel.use_credentials = true;
        let keys = vec![
            KeyCandidate {
                id: 1,
                channel_id: channel.id,
                credential_id: Some(1),
                secret_ciphertext: "blocked".to_string(),
                cooldown_until: None,
                plan_type: Some("free".to_string()),
                plan_models: vec![PlanModel {
                    protocol: UpstreamProtocol::OpenAiOauth,
                    model: "gpt-5.4".to_string(),
                }],
            },
            KeyCandidate {
                id: 2,
                channel_id: channel.id,
                credential_id: Some(2),
                secret_ciphertext: "available".to_string(),
                cooldown_until: None,
                plan_type: Some("free".to_string()),
                plan_models: vec![PlanModel {
                    protocol: UpstreamProtocol::OpenAiOauth,
                    model: "gpt-5.4".to_string(),
                }],
            },
        ];
        let now = Utc::now();
        let mut model_blocks = HashMap::new();
        model_blocks.insert(
            ModelBlockKey {
                protocol: UpstreamProtocol::OpenAiOauth,
                endpoint_id: channel.endpoint_id,
                channel_key_id: None,
                credential_id: Some(1),
                model: "gpt-5.4".to_string(),
            },
            now + chrono::Duration::hours(1),
        );
        let empty_blocks = HashMap::new();
        let model_blocks = ModelBlockLookup::new(&model_blocks, &empty_blocks);

        assert_eq!(
            choose_key(&channel, &keys, "gpt-5.4", now, &model_blocks)
                .unwrap()
                .secret_ciphertext,
            "available"
        );
    }

    #[tokio::test]
    async fn key_selection_skips_model_outside_credential_plan() {
        let mut channel = candidate("oauth", 0, 1, vec!["gpt-5.4"]);
        channel.protocol = UpstreamProtocol::OpenAiOauth;
        channel.use_credentials = true;
        let keys = vec![
            KeyCandidate {
                id: 1,
                channel_id: channel.id,
                credential_id: Some(1),
                secret_ciphertext: "wrong-plan".to_string(),
                cooldown_until: None,
                plan_type: Some("free".to_string()),
                plan_models: vec![PlanModel {
                    protocol: UpstreamProtocol::OpenAiOauth,
                    model: "gpt-5.2".to_string(),
                }],
            },
            KeyCandidate {
                id: 2,
                channel_id: channel.id,
                credential_id: Some(2),
                secret_ciphertext: "right-plan".to_string(),
                cooldown_until: None,
                plan_type: Some("plus".to_string()),
                plan_models: vec![PlanModel {
                    protocol: UpstreamProtocol::OpenAiOauth,
                    model: "gpt-5.4".to_string(),
                }],
            },
        ];

        assert_eq!(
            choose_key(
                &channel,
                &keys,
                "gpt-5.4",
                Utc::now(),
                &empty_block_lookup()
            )
            .unwrap()
            .secret_ciphertext,
            "right-plan"
        );
    }
}
