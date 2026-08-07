use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, RwLock as StdRwLock,
    },
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::{Mutex, RwLock};

use crate::{
    admin::channel::KeySelectionMode,
    error::{AppError, AppResult},
    id::DbId,
    secrets::SecretStore,
};

use super::affinity::{
    ChannelAffinityKey, ChannelAffinityObservation, ChannelAffinityStatus, UpstreamAffinityTarget,
};

mod cache;
mod choose;
mod load;
mod responses;

use choose::{
    channel_is_available, channel_keys, choose_channel_for_request, choose_key, key_is_available,
    matching_channel_count, unavailable_channel_message, was_attempted, ChannelAvailability,
};
#[cfg(test)]
use choose::{channel_matches_model, choose_channel, choose_channel_by_slot};
#[cfg(test)]
use load::build_route_indexes;
use load::{credential_runtime_secret, load_routing_cache};
use responses::ResponsesSupportCache;

const RUNTIME_SECRET_CACHE_MAX_ENTRIES: usize = 4096;

type RouteIndex = HashMap<UpstreamProtocol, HashMap<String, Vec<usize>>>;
type WildcardRouteIndex = HashMap<UpstreamProtocol, Vec<usize>>;

#[derive(Clone, Copy, Default)]
pub(crate) struct SelectionConstraints<'a> {
    pub(crate) affinity_key: Option<&'a ChannelAffinityKey>,
    pub(crate) attempted: &'a [AttemptedUpstream],
    pub(crate) excluded_endpoint_ids: &'a [DbId],
}

struct SelectionScope<'a> {
    secrets: &'a SecretStore,
    snapshot: &'a RoutingCache,
    protocol: UpstreamProtocol,
    model: &'a str,
    now: DateTime<Utc>,
    model_blocks: ModelBlockLookup<'a>,
    attempted: &'a [AttemptedUpstream],
    excluded_endpoint_ids: &'a [DbId],
}

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
    /// 每次 invalidate 递增。routing_snapshot 在加载前捕获、写回前比对：若期间
    /// 发生过 invalidate，则本次加载视为陈旧（loaded_at 置 None），下次强制重载。
    /// 解决 reload 用陈旧 DB 数据覆盖 invalidate 意图的 TOCTOU 竞态。
    routing_epoch: Arc<AtomicU64>,
    routing_cache_ttl: Duration,
    credential_runtime_secrets: RuntimeSecretCache,
    model_blocks: ModelBlockCache,
    responses_support: ResponsesSupportCache,
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

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct ModelBlockKey {
    protocol: UpstreamProtocol,
    endpoint_id: DbId,
    channel_key_id: Option<DbId>,
    credential_id: Option<DbId>,
    model: String,
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
    /// 显式 adapter 类型 hint，从 channel_endpoint.adapter_hint 加载。
    /// None 表示按 provider 默认规则选取。
    pub adapter_hint: Option<String>,
    /// 运行时降级标志：为 true 时，本条 responses 请求由 adapter 改走 /v1/chat/completions
    /// 并用 bridge 把响应转回 responses 格式。由 per-(endpoint,model) 学习写入，不从 DB 加载。
    pub responses_chat_fallback: bool,
    pub secret: String,
    pub account_id: Option<String>,
    pub(crate) affinity: Option<ChannelAffinityObservation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttemptedUpstream {
    pub channel_id: DbId,
    pub channel_endpoint_id: DbId,
    pub channel_key_id: Option<DbId>,
    pub credential_id: Option<DbId>,
}

impl From<&SelectedUpstream> for AttemptedUpstream {
    fn from(upstream: &SelectedUpstream) -> Self {
        Self {
            channel_id: upstream.channel_id,
            channel_endpoint_id: upstream.channel_endpoint_id,
            channel_key_id: upstream.channel_key_id,
            credential_id: upstream.credential_id,
        }
    }
}

pub struct ModelCooldown<'a> {
    pub unavailable_until: DateTime<Utc>,
    pub last_error: &'a str,
    pub last_status_code: i32,
}

#[derive(Debug, Clone)]
pub struct ChannelCandidate {
    pub id: DbId,
    pub endpoint_id: DbId,
    pub protocol: UpstreamProtocol,
    pub provider: String,
    pub name: String,
    pub base_url: String,
    pub adapter_hint: Option<String>,
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
    local: &'a ModelBlockCache,
}

impl<'a> ModelBlockLookup<'a> {
    fn new(
        persisted: &'a HashMap<ModelBlockKey, DateTime<Utc>>,
        local: &'a ModelBlockCache,
    ) -> Self {
        Self { persisted, local }
    }

    fn contains_active(&self, key: &ModelBlockKey, now: DateTime<Utc>) -> bool {
        self.persisted
            .get(key)
            .is_some_and(|blocked_until| *blocked_until > now)
            || self.local.contains_active(key, now)
    }

    fn is_empty(&self) -> bool {
        self.persisted.is_empty() && self.local.is_empty()
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
            routing_epoch: Arc::new(AtomicU64::new(0)),
            routing_cache_ttl,
            credential_runtime_secrets: RuntimeSecretCache::default(),
            model_blocks: ModelBlockCache::default(),
            responses_support: ResponsesSupportCache::default(),
        }
    }

    pub async fn invalidate(&self) {
        let mut cache = self.routing_cache.write().await;
        // 在持写锁时递增 epoch，与 routing_snapshot 的写回串行化：
        // 若 reload 在本次之前捕获了旧 epoch，其写回时会检测到不一致并作废。
        self.routing_epoch.fetch_add(1, Ordering::AcqRel);
        let mut next = (**cache).clone();
        next.loaded_at = None;
        *cache = Arc::new(next);
        drop(cache);
        self.credential_runtime_secrets.clear();
        self.model_blocks.clear_expired(Utc::now());
        self.responses_support.clear_expired(Utc::now());
    }

    /// 该 (endpoint, model) 是否已学习为「不支持 /v1/responses」。仅在 responses 路由决策处查询。
    pub async fn responses_unsupported(&self, endpoint_id: DbId, model: &str) -> bool {
        self.responses_support
            .is_unsupported(endpoint_id, model, Utc::now())
    }

    /// 标记某 (endpoint, model) 不支持 responses，到期后自愈。
    pub async fn mark_responses_unsupported(
        &self,
        endpoint_id: DbId,
        model: &str,
        unsupported_until: DateTime<Utc>,
    ) {
        self.responses_support
            .mark_unsupported(endpoint_id, model, unsupported_until);
    }

    pub(crate) async fn responses_unsupported_endpoint_ids(
        &self,
        pool: &PgPool,
        protocols: &[UpstreamProtocol],
        model: &str,
    ) -> AppResult<Vec<DbId>> {
        let snapshot = self.routing_snapshot(pool).await?;
        let now = Utc::now();
        Ok(snapshot
            .channels
            .iter()
            .filter(|channel| protocols.contains(&channel.protocol))
            .filter(|channel| choose::channel_matches_model(channel, model))
            .filter(|channel| {
                self.responses_support
                    .is_unsupported(channel.endpoint_id, model, now)
            })
            .map(|channel| channel.endpoint_id)
            .collect())
    }

    pub async fn invalidate_refreshed_credential(&self, credential_id: DbId) {
        self.credential_runtime_secrets.remove(credential_id);
        let mut cache = self.routing_cache.write().await;
        self.routing_epoch.fetch_add(1, Ordering::AcqRel);
        let mut next = (**cache).clone();
        next.loaded_at = None;
        *cache = Arc::new(next);
    }

    pub async fn select(
        &self,
        pool: &PgPool,
        secrets: &SecretStore,
        protocol: UpstreamProtocol,
        model: &str,
    ) -> AppResult<SelectedUpstream> {
        let snapshot = self.routing_snapshot(pool).await?;
        let scope = SelectionScope {
            secrets,
            snapshot: &snapshot,
            protocol,
            model,
            now: Utc::now(),
            model_blocks: ModelBlockLookup::new(&snapshot.model_blocks, &self.model_blocks),
            attempted: &[],
            excluded_endpoint_ids: &[],
        };
        self.select_from_snapshot(&scope)
    }

    pub(crate) async fn select_matching_endpoint(
        &self,
        pool: &PgPool,
        secrets: &SecretStore,
        protocol: UpstreamProtocol,
        model: &str,
        channel_id: Option<DbId>,
        matches: impl Fn(&ChannelCandidate) -> bool,
    ) -> AppResult<SelectedUpstream> {
        let snapshot = self.routing_snapshot(pool).await?;
        let now = Utc::now();
        let model_blocks = ModelBlockLookup::new(&snapshot.model_blocks, &self.model_blocks);
        let channel = choose::choose_channel_for_request_matching(
            &snapshot,
            protocol,
            model,
            now,
            &model_blocks,
            &[],
            None,
            |channel| channel_id.is_none_or(|id| channel.id == id) && matches(channel),
        )
        .ok_or_else(|| {
            AppError::UpstreamUnavailable(format!(
                "no available matching upstream channel for {model}"
            ))
        })?;
        let keys = channel_keys(&snapshot, channel);
        let key = choose_key(channel, keys, model, now, &model_blocks, &[]).ok_or_else(|| {
            AppError::UpstreamUnavailable(format!("channel {} has no available key", channel.name))
        })?;
        self.selected_upstream_from_candidate(secrets, channel, key)
    }

    pub(crate) async fn select_exact_endpoint(
        &self,
        pool: &PgPool,
        secrets: &SecretStore,
        protocol: UpstreamProtocol,
        model: &str,
        endpoint_id: DbId,
        channel_key_id: Option<DbId>,
        credential_id: Option<DbId>,
    ) -> AppResult<SelectedUpstream> {
        let snapshot = self.routing_snapshot(pool).await?;
        let now = Utc::now();
        let model_blocks = ModelBlockLookup::new(&snapshot.model_blocks, &self.model_blocks);
        let channel = snapshot
            .channels
            .iter()
            .find(|channel| channel.endpoint_id == endpoint_id && channel.protocol == protocol)
            .ok_or_else(|| {
                AppError::UpstreamUnavailable(
                    "bound asset upstream endpoint is no longer available".to_string(),
                )
            })?;
        let availability = ChannelAvailability {
            protocol,
            model,
            now,
            model_blocks: &model_blocks,
            attempted: &[],
            excluded_endpoint_ids: None,
        };
        if !channel_is_available(&snapshot, channel, &availability) {
            return Err(AppError::UpstreamUnavailable(
                "bound asset upstream endpoint is not available for this model".to_string(),
            ));
        }
        let key = channel_keys(&snapshot, channel)
            .iter()
            .find(|key| {
                if channel.use_credentials {
                    credential_id.is_some_and(|id| key.credential_id == Some(id))
                } else {
                    channel_key_id.is_some_and(|id| key.id == id && key.credential_id.is_none())
                }
            })
            .filter(|key| key_is_available(channel, key, model, now, &model_blocks))
            .ok_or_else(|| {
                AppError::UpstreamUnavailable(
                    "bound asset upstream key is no longer available".to_string(),
                )
            })?;
        self.selected_upstream_from_candidate(secrets, channel, key)
    }

    pub(crate) async fn select_with_affinity(
        &self,
        pool: &PgPool,
        secrets: &SecretStore,
        affinity_cache: &super::affinity::ChannelAffinityCache,
        protocol: UpstreamProtocol,
        model: &str,
        constraints: SelectionConstraints<'_>,
    ) -> AppResult<SelectedUpstream> {
        self.select_with_affinity_excluding(
            pool,
            secrets,
            affinity_cache,
            protocol,
            model,
            constraints,
        )
        .await
    }

    pub(crate) async fn select_with_affinity_excluding(
        &self,
        pool: &PgPool,
        secrets: &SecretStore,
        affinity_cache: &super::affinity::ChannelAffinityCache,
        protocol: UpstreamProtocol,
        model: &str,
        constraints: SelectionConstraints<'_>,
    ) -> AppResult<SelectedUpstream> {
        let snapshot = self.routing_snapshot(pool).await?;
        let scope = SelectionScope {
            secrets,
            snapshot: &snapshot,
            protocol,
            model,
            now: Utc::now(),
            model_blocks: ModelBlockLookup::new(&snapshot.model_blocks, &self.model_blocks),
            attempted: constraints.attempted,
            excluded_endpoint_ids: constraints.excluded_endpoint_ids,
        };
        if let Some(affinity_key) = constraints.affinity_key {
            if let Some((target, status)) = affinity_cache.get(affinity_key).await {
                if let Some(mut upstream) = self.selected_affinity_upstream(&scope, &target)? {
                    upstream.affinity = Some(ChannelAffinityObservation {
                        status,
                        key_fingerprint: affinity_key.fingerprint(),
                    });
                    tracing::debug!(
                        rule = affinity_key.rule,
                        protocol = protocol.as_str(),
                        model,
                        channel_id = upstream.channel_id,
                        channel_endpoint_id = upstream.channel_endpoint_id,
                        channel_key_id = ?upstream.channel_key_id,
                        credential_id = ?upstream.credential_id,
                        "selected upstream from channel affinity cache"
                    );
                    return Ok(upstream);
                }
            }
        }

        let mut upstream = self.select_from_snapshot(&scope)?;
        if affinity_cache.enabled() && constraints.affinity_key.is_some() {
            upstream.affinity = constraints
                .affinity_key
                .map(|key| ChannelAffinityObservation {
                    status: ChannelAffinityStatus::Miss,
                    key_fingerprint: key.fingerprint(),
                });
        }
        Ok(upstream)
    }

    pub(crate) async fn select_with_affinity_excluding_protocols(
        &self,
        pool: &PgPool,
        secrets: &SecretStore,
        affinity_cache: &super::affinity::ChannelAffinityCache,
        protocols: &[UpstreamProtocol],
        model: &str,
        constraints: SelectionConstraints<'_>,
    ) -> AppResult<(UpstreamProtocol, SelectedUpstream)> {
        let snapshot = self.routing_snapshot(pool).await?;
        let now = Utc::now();
        let mut last_unavailable = None;

        for &protocol in protocols {
            let scope = SelectionScope {
                secrets,
                snapshot: &snapshot,
                protocol,
                model,
                now,
                model_blocks: ModelBlockLookup::new(&snapshot.model_blocks, &self.model_blocks),
                attempted: constraints.attempted,
                excluded_endpoint_ids: constraints.excluded_endpoint_ids,
            };

            if let Some(affinity_key) = constraints.affinity_key {
                if let Some((target, status)) = affinity_cache.get(affinity_key).await {
                    if let Some(mut upstream) = self.selected_affinity_upstream(&scope, &target)? {
                        upstream.affinity = Some(ChannelAffinityObservation {
                            status,
                            key_fingerprint: affinity_key.fingerprint(),
                        });
                        tracing::debug!(
                            rule = affinity_key.rule,
                            protocol = protocol.as_str(),
                            model,
                            channel_id = upstream.channel_id,
                            channel_endpoint_id = upstream.channel_endpoint_id,
                            channel_key_id = ?upstream.channel_key_id,
                            credential_id = ?upstream.credential_id,
                            "selected upstream from channel affinity cache"
                        );
                        return Ok((protocol, upstream));
                    }
                }
            }

            match self.select_from_snapshot(&scope) {
                Ok(mut upstream) => {
                    if affinity_cache.enabled() && constraints.affinity_key.is_some() {
                        upstream.affinity =
                            constraints
                                .affinity_key
                                .map(|key| ChannelAffinityObservation {
                                    status: ChannelAffinityStatus::Miss,
                                    key_fingerprint: key.fingerprint(),
                                });
                    }
                    return Ok((protocol, upstream));
                }
                Err(AppError::UpstreamUnavailable(message)) => {
                    last_unavailable = Some(message);
                }
                Err(err) => return Err(err),
            }
        }

        Err(AppError::UpstreamUnavailable(
            last_unavailable
                .unwrap_or_else(|| format!("no available upstream channel for {model}")),
        ))
    }

    pub(crate) async fn select_bound_channel_protocols(
        &self,
        pool: &PgPool,
        secrets: &SecretStore,
        protocols: &[UpstreamProtocol],
        model: &str,
        channel_id: DbId,
        constraints: SelectionConstraints<'_>,
    ) -> AppResult<(UpstreamProtocol, SelectedUpstream)> {
        let snapshot = self.routing_snapshot(pool).await?;
        let now = Utc::now();
        let mut last_unavailable = None;

        for &protocol in protocols {
            let model_blocks = ModelBlockLookup::new(&snapshot.model_blocks, &self.model_blocks);
            let availability = ChannelAvailability {
                protocol,
                model,
                now,
                model_blocks: &model_blocks,
                attempted: constraints.attempted,
                excluded_endpoint_ids: Some(constraints.excluded_endpoint_ids),
            };
            let channels = snapshot
                .channels
                .iter()
                .filter(|channel| channel.id == channel_id && channel.protocol == protocol);
            for channel in channels {
                if !channel_is_available(&snapshot, channel, &availability) {
                    continue;
                }
                let keys = channel_keys(&snapshot, channel);
                if let Some(key) = choose_key(
                    channel,
                    keys,
                    model,
                    now,
                    &model_blocks,
                    constraints.attempted,
                ) {
                    return Ok((
                        protocol,
                        self.selected_upstream_from_candidate(secrets, channel, key)?,
                    ));
                }
            }
            last_unavailable = Some(unavailable_channel_message(
                &snapshot,
                protocol,
                model,
                now,
                &model_blocks,
            ));
        }

        Err(AppError::UpstreamUnavailable(
            last_unavailable.unwrap_or_else(|| {
                format!("no available upstream channel {channel_id} for {model}")
            }),
        ))
    }

    fn selected_affinity_upstream(
        &self,
        scope: &SelectionScope<'_>,
        target: &UpstreamAffinityTarget,
    ) -> AppResult<Option<SelectedUpstream>> {
        let Some(channel) = scope.snapshot.channels.iter().find(|channel| {
            channel.id == target.channel_id && channel.endpoint_id == target.channel_endpoint_id
        }) else {
            return Ok(None);
        };
        if !channel_is_available(
            scope.snapshot,
            channel,
            &ChannelAvailability {
                protocol: scope.protocol,
                model: scope.model,
                now: scope.now,
                model_blocks: &scope.model_blocks,
                attempted: scope.attempted,
                excluded_endpoint_ids: Some(scope.excluded_endpoint_ids),
            },
        ) {
            return Ok(None);
        }
        let keys = channel_keys(scope.snapshot, channel);
        let Some(key) = keys.iter().find(|key| {
            key.credential_id == target.credential_id
                && (!channel.use_credentials).then_some(key.id) == target.channel_key_id
                && key_is_available(channel, key, scope.model, scope.now, &scope.model_blocks)
                && !was_attempted(channel, key, scope.attempted)
        }) else {
            return Ok(None);
        };
        Ok(Some(self.selected_upstream_from_candidate(
            scope.secrets,
            channel,
            key,
        )?))
    }

    fn select_from_snapshot(&self, scope: &SelectionScope<'_>) -> AppResult<SelectedUpstream> {
        let channel = choose_channel_for_request(
            scope.snapshot,
            scope.protocol,
            scope.model,
            scope.now,
            &scope.model_blocks,
            scope.attempted,
            Some(scope.excluded_endpoint_ids),
        )
        .ok_or_else(|| {
            AppError::UpstreamUnavailable(unavailable_channel_message(
                scope.snapshot,
                scope.protocol,
                scope.model,
                scope.now,
                &scope.model_blocks,
            ))
        })?;
        let keys = channel_keys(scope.snapshot, channel);
        let key = choose_key(
            channel,
            keys,
            scope.model,
            scope.now,
            &scope.model_blocks,
            scope.attempted,
        )
        .ok_or_else(|| {
            AppError::UpstreamUnavailable(format!("channel {} has no available key", channel.name))
        })?;
        self.selected_upstream_from_candidate(scope.secrets, channel, key)
    }

    fn selected_upstream_from_candidate(
        &self,
        secrets: &SecretStore,
        channel: &ChannelCandidate,
        key: &KeyCandidate,
    ) -> AppResult<SelectedUpstream> {
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
            adapter_hint: channel.adapter_hint.clone(),
            responses_chat_fallback: false,
            secret: runtime.secret,
            account_id: runtime.account_id,
            affinity: None,
        })
    }

    pub(crate) async fn mark_model_unavailable_local(
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

    pub(crate) async fn mark_credential_model_unavailable(
        &self,
        pool: &PgPool,
        upstream: &SelectedUpstream,
        protocol: UpstreamProtocol,
        model: &str,
        cooldown: ModelCooldown<'_>,
    ) -> AppResult<bool> {
        if !self
            .has_alternate_channel_for_model(pool, protocol, model)
            .await?
        {
            return Ok(false);
        }
        self.mark_model_unavailable_local(upstream, protocol, model, cooldown.unavailable_until)
            .await;
        sqlx::query(
            "UPDATE channel_model
             SET runtime_status = 'cooldown',
                 cooldown_until = $3,
                 last_error = $4,
                 last_status_code = $5,
                 failure_count = failure_count + 1,
                 updated_at = now()
             WHERE channel_id = $1
               AND model = $2",
        )
        .bind(upstream.channel_id)
        .bind(model)
        .bind(cooldown.unavailable_until)
        .bind(cooldown.last_error.chars().take(500).collect::<String>())
        .bind(cooldown.last_status_code)
        .execute(pool)
        .await?;
        Ok(true)
    }

    pub(crate) async fn mark_model_available(
        &self,
        pool: &PgPool,
        upstream: &SelectedUpstream,
        model: &str,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE channel_model
             SET runtime_status = 'normal',
                 cooldown_until = NULL,
                 last_error = NULL,
                 last_status_code = NULL,
                 last_probe_at = now(),
                 success_count = success_count + 1,
                 updated_at = now()
             WHERE channel_id = $1
               AND model = $2",
        )
        .bind(upstream.channel_id)
        .bind(model)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn mark_key_failure_local(
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

        // 在读 DB 前捕获 epoch。DB 读期间若发生 invalidate（不持 reload_lock，只持
        // cache 写锁），本次加载可能已陈旧。
        let epoch_before = self.routing_epoch.load(Ordering::Acquire);
        let mut loaded = load_routing_cache(pool).await?;
        let mut cache = self.routing_cache.write().await;
        if self.routing_epoch.load(Ordering::Acquire) != epoch_before {
            // 加载期间被 invalidate：仍存入作为当前可用值，但标记为非 fresh，
            // 使下次 routing_snapshot 强制重载，不让陈旧数据抹除 invalidate 的意图。
            loaded.loaded_at = None;
        }
        let loaded = Arc::new(loaded);
        *cache = Arc::clone(&loaded);
        Ok(loaded)
    }

    pub(crate) async fn has_alternate_channel_for_model(
        &self,
        pool: &PgPool,
        protocol: UpstreamProtocol,
        model: &str,
    ) -> AppResult<bool> {
        let snapshot = self.routing_snapshot(pool).await?;
        Ok(matching_channel_count(&snapshot, protocol, model) > 1)
    }

    pub(crate) async fn has_selectable_upstream_excluding(
        &self,
        pool: &PgPool,
        protocol: UpstreamProtocol,
        model: &str,
        attempted: &[AttemptedUpstream],
    ) -> AppResult<bool> {
        let snapshot = self.routing_snapshot(pool).await?;
        let now = Utc::now();
        let model_blocks = ModelBlockLookup::new(&snapshot.model_blocks, &self.model_blocks);
        let Some(channel) = choose_channel_for_request(
            &snapshot,
            protocol,
            model,
            now,
            &model_blocks,
            attempted,
            None,
        ) else {
            return Ok(false);
        };
        let keys = channel_keys(&snapshot, channel);
        Ok(choose_key(channel, keys, model, now, &model_blocks, attempted).is_some())
    }
}

#[cfg(test)]
mod tests;
