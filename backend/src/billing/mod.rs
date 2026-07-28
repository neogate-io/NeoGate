use std::{
    collections::hash_map::DefaultHasher,
    fmt,
    hash::{Hash, Hasher},
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use sqlx::{PgPool, Postgres, Row, Transaction};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    id::DbId,
};

pub(crate) mod account;
mod credit;
mod metering;
pub mod outbox;
mod types;
pub mod video;

use credit::{HotAllocation, HotCreditStore, MemoryHotCreditStore, RedisHotCreditStore};
pub use metering::{
    cost_for_billable_usage, estimate_input_tokens, estimated_cost_micros, parse_usage_from_bytes,
    parse_usage_from_sse_data,
};
pub use types::{
    BillableUsage, BillingCharge, BillingMeter, CreditAccountId, CreditAccountType, DebitHold,
    DebitPart, Price, PricingBasis, TokenUsage, VideoBillingMode, VideoPriceTier,
};

pub const MICROS_PER_MAJOR_UNIT: i64 = 1_000_000;
pub const BILLABLE_PRICE_CONDITION: &str = r#"
(
    (billing_meter = 'token'
        AND input_price_micros >= 0
        AND output_price_micros >= 0)
    OR (billing_meter = 'image'
        AND unit_price_micros > 0)
    OR (billing_meter = 'video'
        AND video_billing_mode IS NOT NULL
        AND CASE
            WHEN jsonb_typeof(video_price_tiers) = 'array'
            THEN jsonb_array_length(video_price_tiers) > 0
            ELSE FALSE
        END)
)
"#;
pub const BILLABLE_PRICE_CONDITION_CP: &str = r#"
(
    (cp.billing_meter = 'token'
        AND cp.input_price_micros >= 0
        AND cp.output_price_micros >= 0)
    OR (cp.billing_meter = 'image'
        AND cp.unit_price_micros > 0)
    OR (cp.billing_meter = 'video'
        AND cp.video_billing_mode IS NOT NULL
        AND CASE
            WHEN jsonb_typeof(cp.video_price_tiers) = 'array'
            THEN jsonb_array_length(cp.video_price_tiers) > 0
            ELSE FALSE
        END)
)
"#;
const ALLOCATION_RECOVERY_LOG_SAMPLE_LIMIT: usize = 20;

#[derive(Clone)]
pub struct Billing {
    hot: Arc<dyn HotCreditStore>,
    price_cache: PriceCache,
    prefetch_locks: Arc<Vec<Mutex<()>>>,
    prefetch_micros: i64,
    default_output_tokens: i64,
}

#[derive(Clone, Copy)]
pub struct BillingAccounts<'a> {
    pub user_id: DbId,
    pub project_id: DbId,
    pub user_key_id: DbId,
    pub user_key_model_credit_account: Option<&'a CreditAccountId>,
    pub user_key_credit_account: &'a CreditAccountId,
    pub project_credit_account: &'a CreditAccountId,
}

pub struct SettleRequest<'a> {
    pub accounts: BillingAccounts<'a>,
    pub hold: DebitHold,
    pub usage: Option<BillableUsage>,
    pub price: &'a Price,
}

#[derive(Debug, Default)]
struct AllocationRecoverySummary {
    count: u64,
    recovered_micros: i64,
    oldest_created_at: Option<DateTime<Utc>>,
    oldest_age_seconds: Option<i64>,
    samples: Vec<AllocationRecoverySample>,
    truncated: usize,
}

struct AllocationRecoverySample {
    allocation_id: DbId,
    credit_account_id: DbId,
    amount_micros: i64,
    consumed_micros: i64,
    already_returned_micros: i64,
    recovered_micros: i64,
    age_seconds: i64,
}

impl fmt::Debug for AllocationRecoverySample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AllocationRecoverySample")
            .field("allocation_id", &self.allocation_id)
            .field("credit_account_id", &self.credit_account_id)
            .field("amount_micros", &self.amount_micros)
            .field("consumed_micros", &self.consumed_micros)
            .field("already_returned_micros", &self.already_returned_micros)
            .field("recovered_micros", &self.recovered_micros)
            .field("age_seconds", &self.age_seconds)
            .finish()
    }
}

type PriceCacheKey = (DbId, String, String);

#[derive(Clone)]
struct PriceCache {
    ttl: Duration,
    max_entries: usize,
    entries: Arc<DashMap<PriceCacheKey, CachedPrice>>,
}

#[derive(Clone)]
struct CachedPrice {
    price: Price,
    expires_at: Instant,
}

impl Billing {
    pub fn new_memory(
        price_cache_ttl: Duration,
        price_cache_max_entries: usize,
        prefetch_micros: i64,
        default_output_tokens: i64,
    ) -> Self {
        Self {
            hot: Arc::new(MemoryHotCreditStore::default()),
            price_cache: PriceCache::new(price_cache_ttl, price_cache_max_entries),
            prefetch_locks: prefetch_locks(),
            prefetch_micros,
            default_output_tokens,
        }
    }

    pub async fn new_redis(
        redis_url: &str,
        key_prefix: String,
        price_cache_ttl: Duration,
        price_cache_max_entries: usize,
        prefetch_micros: i64,
        default_output_tokens: i64,
    ) -> AppResult<Self> {
        Ok(Self {
            hot: Arc::new(RedisHotCreditStore::connect(redis_url, key_prefix).await?),
            price_cache: PriceCache::new(price_cache_ttl, price_cache_max_entries),
            prefetch_locks: prefetch_locks(),
            prefetch_micros,
            default_output_tokens,
        })
    }

    pub fn default_output_tokens(&self) -> i64 {
        self.default_output_tokens
    }

    pub async fn price_for(
        &self,
        pool: &PgPool,
        channel_id: DbId,
        model: &str,
        user_group: &str,
    ) -> AppResult<Price> {
        self.price_cache
            .price_for(pool, channel_id, model, user_group)
            .await
    }

    pub fn invalidate_price(&self, channel_id: DbId, model: &str) {
        self.price_cache.invalidate(channel_id, model);
    }

    pub fn invalidate_all_prices(&self) {
        self.price_cache.invalidate_all();
    }

    pub async fn drain_hot_credit_account(
        &self,
        credit_account: &CreditAccountId,
    ) -> AppResult<Vec<DebitPart>> {
        self.hot.drain_credit_account(credit_account).await
    }

    pub fn spawn_allocation_recovery(
        &self,
        pool: PgPool,
        interval: Duration,
        recover_after: Duration,
    ) {
        let billing = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval.max(Duration::from_secs(1)));
            loop {
                ticker.tick().await;
                let Ok(recover_after) = chrono::Duration::from_std(recover_after) else {
                    tracing::warn!("invalid credit allocation recovery window");
                    continue;
                };
                let stale_before = Utc::now() - recover_after;
                match billing.recover_stale_allocations(&pool, stale_before).await {
                    Ok(summary) if summary.count > 0 => {
                        tracing::info!(
                            count = summary.count,
                            recovered_micros = summary.recovered_micros,
                            oldest_created_at = ?summary.oldest_created_at,
                            oldest_age_seconds = summary.oldest_age_seconds,
                            samples = ?summary.samples,
                            truncated = summary.truncated,
                            "recovered stale credit allocations"
                        );
                    }
                    Ok(_) => {}
                    Err(err) => tracing::warn!("failed to recover stale credit allocations: {err}"),
                }
            }
        });
    }

    async fn recover_stale_allocations(
        &self,
        pool: &PgPool,
        stale_before: DateTime<Utc>,
    ) -> AppResult<AllocationRecoverySummary> {
        let allocations = fetch_stale_allocations(pool, stale_before).await?;
        if allocations.is_empty() {
            return Ok(AllocationRecoverySummary::default());
        }

        self.hot.remove_allocations(&allocations).await?;
        recover_allocations_in_db(pool, &allocations).await
    }

    pub async fn reserve(
        &self,
        pool: &PgPool,
        accounts: BillingAccounts<'_>,
        estimated_micros: i64,
    ) -> AppResult<DebitHold> {
        if estimated_micros <= 0 {
            return Ok(DebitHold {
                transaction_id: Uuid::new_v4(),
                estimated_micros: 0,
                parts: Vec::new(),
                charge_credit: true,
            });
        }

        let credit_accounts = ordered_credit_accounts(accounts);
        if let Some(parts) = self
            .hot
            .try_debit_ordered(&credit_accounts, estimated_micros)
            .await?
        {
            return Ok(DebitHold {
                transaction_id: Uuid::new_v4(),
                estimated_micros,
                parts,
                charge_credit: true,
            });
        }

        let lock_id = prefetch_lock_index(accounts, self.prefetch_locks.len());
        let _prefetch_guard = self.prefetch_locks[lock_id].lock().await;

        if let Some(parts) = self
            .hot
            .try_debit_ordered(&credit_accounts, estimated_micros)
            .await?
        {
            return Ok(DebitHold {
                transaction_id: Uuid::new_v4(),
                estimated_micros,
                parts,
                charge_credit: true,
            });
        }

        self.prefetch(pool, accounts, estimated_micros).await?;

        let Some(parts) = self
            .hot
            .try_debit_ordered(&credit_accounts, estimated_micros)
            .await?
        else {
            let available_micros = self.hot.available_micros(&credit_accounts).await?;
            return Err(AppError::InsufficientQuota {
                available_micros,
                required_micros: estimated_micros,
            });
        };

        Ok(DebitHold {
            transaction_id: Uuid::new_v4(),
            estimated_micros,
            parts,
            charge_credit: true,
        })
    }

    pub async fn release_hold(&self, pool: &PgPool, hold: DebitHold) -> AppResult<()> {
        if hold.parts.is_empty() {
            return Ok(());
        }

        let mut tx = pool.begin().await?;
        for part in &hold.parts {
            if part.amount_micros <= 0 {
                continue;
            }
            // If the stale-allocation job already recovered this allocation, the
            // held credit was already returned to the account and `reserved` was
            // already released at recovery time. Releasing again would fail the
            // capacity constraint and double-release reserved credit. Skip it —
            // the hold is already settled. See `flush_billing_part`.
            if account::allocation_is_recovered(&mut tx, part.allocation_id).await? {
                tracing::info!(
                    allocation_id = %part.allocation_id,
                    amount_micros = part.amount_micros,
                    "skipping hold release on already-recovered credit allocation; credit was already refunded"
                );
                continue;
            }
            account::decrement_reserved(&mut tx, &part.credit_account, part.amount_micros).await?;
            account::mark_allocation_returned(&mut tx, part.allocation_id, part.amount_micros)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn settle(
        &self,
        pool: &PgPool,
        request: SettleRequest<'_>,
    ) -> AppResult<BillingCharge> {
        let SettleRequest {
            accounts,
            hold,
            usage,
            price,
        } = request;
        let (cost_micros, status) = match usage {
            Some(usage) => (cost_for_billable_usage(usage, price), "billed".to_string()),
            None => (hold.estimated_micros, "usage_missing".to_string()),
        };
        let cost_micros = cost_micros.max(0);
        let token_usage = usage.and_then(|usage| usage.token_usage);
        let billing_meter = usage.map_or(price.billing_meter, |usage| usage.meter);
        let billable_units = usage.map_or(0, |usage| usage.billable_units.max(0));

        if !hold.charge_credit {
            return Ok(BillingCharge {
                transaction_id: hold.transaction_id,
                input_tokens: token_usage.map(|usage| usage.input_tokens),
                output_tokens: token_usage.map(|usage| usage.output_tokens),
                total_tokens: token_usage.map(TokenUsage::total_tokens),
                billing_meter,
                billable_units,
                cost_micros,
                status,
                parts: Vec::new(),
                returned_parts: Vec::new(),
            });
        }

        if cost_micros >= hold.estimated_micros {
            if cost_micros > hold.estimated_micros {
                let supplemental_micros = cost_micros - hold.estimated_micros;
                match self.reserve(pool, accounts, supplemental_micros).await {
                    Ok(extra_hold) => {
                        let mut parts = hold.parts;
                        parts.extend(extra_hold.parts);
                        return Ok(BillingCharge {
                            transaction_id: hold.transaction_id,
                            input_tokens: token_usage.map(|usage| usage.input_tokens),
                            output_tokens: token_usage.map(|usage| usage.output_tokens),
                            total_tokens: token_usage.map(TokenUsage::total_tokens),
                            billing_meter,
                            billable_units,
                            cost_micros,
                            status,
                            parts,
                            returned_parts: Vec::new(),
                        });
                    }
                    Err(err) => {
                        tracing::warn!(
                            "failed to reserve supplemental billing credit; charging reserved estimate only: {err}"
                        );
                    }
                }
            }
            return Ok(BillingCharge {
                transaction_id: hold.transaction_id,
                input_tokens: token_usage.map(|usage| usage.input_tokens),
                output_tokens: token_usage.map(|usage| usage.output_tokens),
                total_tokens: token_usage.map(TokenUsage::total_tokens),
                billing_meter,
                billable_units,
                cost_micros: hold.estimated_micros,
                status: if cost_micros > hold.estimated_micros {
                    "undercharged".to_string()
                } else {
                    status
                },
                parts: hold.parts,
                returned_parts: Vec::new(),
            });
        }

        let mut remaining = cost_micros;
        let mut consumed = Vec::new();
        let mut returned_parts = Vec::new();
        for part in hold.parts {
            if remaining <= 0 {
                returned_parts.push(part);
                continue;
            }
            let consume = part.amount_micros.min(remaining);
            remaining -= consume;
            consumed.push(DebitPart {
                amount_micros: consume,
                ..part.clone()
            });
            if part.amount_micros > consume {
                returned_parts.push(DebitPart {
                    amount_micros: part.amount_micros - consume,
                    ..part
                });
            }
        }

        Ok(BillingCharge {
            transaction_id: hold.transaction_id,
            input_tokens: token_usage.map(|usage| usage.input_tokens),
            output_tokens: token_usage.map(|usage| usage.output_tokens),
            total_tokens: token_usage.map(TokenUsage::total_tokens),
            billing_meter,
            billable_units,
            cost_micros,
            status,
            parts: consumed,
            returned_parts,
        })
    }

    async fn prefetch(
        &self,
        pool: &PgPool,
        accounts: BillingAccounts<'_>,
        needed_micros: i64,
    ) -> AppResult<()> {
        let mut tx = pool.begin().await?;
        let mut remaining = needed_micros;
        let target = self.prefetch_micros.max(needed_micros);
        let mut allocations = Vec::new();

        if let Some(user_key_model_credit_account) = accounts.user_key_model_credit_account {
            if let Some((allocation_id, amount)) = allocate_user_key_model(
                &mut tx,
                accounts.user_key_id,
                accounts.user_id,
                user_key_model_credit_account,
                target,
            )
            .await?
            {
                allocations.push((
                    HotAllocation {
                        credit_account: user_key_model_credit_account.clone(),
                        allocation_id,
                    },
                    amount,
                ));
                remaining = (remaining - amount).max(0);
            }
        }

        if remaining > 0 {
            let key_target = self.prefetch_micros.max(remaining);
            if let Some((allocation_id, amount)) = allocate_user_key(
                &mut tx,
                accounts.user_key_id,
                accounts.user_id,
                accounts.user_key_credit_account,
                key_target,
            )
            .await?
            {
                allocations.push((
                    HotAllocation {
                        credit_account: accounts.user_key_credit_account.clone(),
                        allocation_id,
                    },
                    amount,
                ));
                remaining = (remaining - amount).max(0);
            }
        }

        if remaining > 0 {
            let project_target = self.prefetch_micros.max(remaining);
            if let Some((allocation_id, amount)) = allocate_project(
                &mut tx,
                accounts.project_id,
                accounts.project_credit_account,
                project_target,
            )
            .await?
            {
                allocations.push((
                    HotAllocation {
                        credit_account: accounts.project_credit_account.clone(),
                        allocation_id,
                    },
                    amount,
                ));
            }
        }

        tx.commit().await?;
        self.credit_allocations(pool, allocations).await?;
        Ok(())
    }

    async fn credit_allocations(
        &self,
        pool: &PgPool,
        allocations: Vec<(HotAllocation, i64)>,
    ) -> AppResult<()> {
        let mut credited = 0;
        for (allocation, amount) in &allocations {
            match self
                .hot
                .credit_allocation(
                    allocation.credit_account.clone(),
                    allocation.allocation_id,
                    *amount,
                )
                .await
            {
                Ok(()) => credited += 1,
                Err(err) => {
                    let uncredited = allocations[credited..]
                        .iter()
                        .map(|(allocation, _)| allocation.clone())
                        .collect::<Vec<_>>();
                    if let Err(recover_err) = recover_allocations_in_db(pool, &uncredited).await {
                        tracing::warn!(
                            "failed to recover uncredited hot allocations after error: {recover_err}"
                        );
                    }
                    return Err(err);
                }
            }
        }
        Ok(())
    }
}

fn prefetch_locks() -> Arc<Vec<Mutex<()>>> {
    Arc::new((0..64).map(|_| Mutex::new(())).collect())
}

fn ordered_credit_accounts(accounts: BillingAccounts<'_>) -> Vec<CreditAccountId> {
    let mut credit_accounts = Vec::with_capacity(3);
    if let Some(credit_account) = accounts.user_key_model_credit_account {
        push_unique_credit_account(&mut credit_accounts, credit_account);
    }
    push_unique_credit_account(&mut credit_accounts, accounts.user_key_credit_account);
    push_unique_credit_account(&mut credit_accounts, accounts.project_credit_account);
    credit_accounts
}

fn push_unique_credit_account(
    credit_accounts: &mut Vec<CreditAccountId>,
    credit_account: &CreditAccountId,
) {
    if !credit_accounts.contains(credit_account) {
        credit_accounts.push(credit_account.clone());
    }
}

fn prefetch_lock_index(accounts: BillingAccounts<'_>, len: usize) -> usize {
    let mut hasher = DefaultHasher::new();
    accounts.user_key_model_credit_account.hash(&mut hasher);
    accounts.user_key_credit_account.hash(&mut hasher);
    accounts.project_credit_account.hash(&mut hasher);
    hasher.finish() as usize % len.max(1)
}

impl PriceCache {
    fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            ttl,
            max_entries,
            entries: Arc::new(DashMap::new()),
        }
    }

    fn invalidate(&self, channel_id: DbId, model: &str) {
        self.entries
            .retain(|(cached_channel_id, cached_model, _), _| {
                *cached_channel_id != channel_id || cached_model != model
            });
    }

    fn invalidate_all(&self) {
        self.entries.clear();
    }

    async fn price_for(
        &self,
        pool: &PgPool,
        channel_id: DbId,
        model: &str,
        user_group: &str,
    ) -> AppResult<Price> {
        let key = (channel_id, model.to_string(), user_group.to_string());
        if let Some(cached) = self.entries.get(&key) {
            if cached.expires_at > Instant::now() {
                return Ok(cached.price.clone());
            }
            drop(cached);
            self.entries.remove(&key);
        }

        let row = sqlx::query(
            r#"
            WITH base_price AS (
                SELECT input_price_micros, output_price_micros,
                       cache_read_price_micros,
                       cache_write_price_micros,
                       billing_meter,
                       unit_price_micros,
                       video_billing_mode,
                       video_price_tiers
                FROM channel_price
                WHERE channel_id = $1 AND model = $2 AND enabled = TRUE
            ),
            policy AS (
                SELECT multiplier_micros
                FROM pricing_policy
                WHERE enabled = TRUE
                  AND user_group = $3
                ORDER BY
                    priority DESC,
                    id DESC
                LIMIT 1
            )
            SELECT
                (base_price.input_price_micros *
                    COALESCE(policy.multiplier_micros, 1000000) + 500000) / 1000000
                    AS input_price_micros,
                (base_price.output_price_micros *
                    COALESCE(policy.multiplier_micros, 1000000) + 500000) / 1000000
                    AS output_price_micros,
                CASE
                    WHEN base_price.cache_read_price_micros IS NULL THEN NULL
                    ELSE (base_price.cache_read_price_micros *
                        COALESCE(policy.multiplier_micros, 1000000) + 500000) / 1000000
                END AS cache_read_price_micros,
                CASE
                    WHEN base_price.cache_write_price_micros IS NULL THEN NULL
                    ELSE (base_price.cache_write_price_micros *
                        COALESCE(policy.multiplier_micros, 1000000) + 500000) / 1000000
                END AS cache_write_price_micros,
                base_price.billing_meter,
                CASE
                    WHEN base_price.unit_price_micros IS NULL THEN NULL
                    ELSE (base_price.unit_price_micros *
                        COALESCE(policy.multiplier_micros, 1000000) + 500000) / 1000000
                END AS unit_price_micros,
                base_price.video_billing_mode,
                base_price.video_price_tiers,
                COALESCE(policy.multiplier_micros, 1000000) AS multiplier_micros
            FROM base_price
            LEFT JOIN policy ON TRUE
            "#,
        )
        .bind(channel_id)
        .bind(model)
        .bind(user_group)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "price is not configured for channel {channel_id}/{model}"
            ))
        })?;

        let multiplier_micros: i64 = row.try_get("multiplier_micros")?;
        let video_billing_mode = row
            .try_get::<Option<String>, _>("video_billing_mode")?
            .map(|value| VideoBillingMode::from_strict_str(&value).map_err(AppError::BadRequest))
            .transpose()?;
        let video_price_tiers: Vec<VideoPriceTier> =
            serde_json::from_value(row.try_get("video_price_tiers")?).map_err(AppError::from)?;
        let price = Price {
            input_price_micros: row.try_get("input_price_micros")?,
            output_price_micros: row.try_get("output_price_micros")?,
            cache_read_price_micros: row.try_get("cache_read_price_micros")?,
            cache_write_price_micros: row.try_get("cache_write_price_micros")?,
            billing_meter: BillingMeter::from_strict_str(
                &row.try_get::<String, _>("billing_meter")?,
            )
            .map_err(AppError::BadRequest)?,
            unit_price_micros: row.try_get("unit_price_micros")?,
            video_billing_mode,
            video_price_tiers: scale_video_price_tiers(video_price_tiers, multiplier_micros),
        };
        if !self.ttl.is_zero() {
            let now = Instant::now();
            if self.entries.len() >= self.max_entries {
                self.entries.retain(|_, cached| cached.expires_at > now);
                trim_price_cache_for_insert(&self.entries, &key, self.max_entries);
            }
            self.entries.insert(
                key,
                CachedPrice {
                    price: price.clone(),
                    expires_at: now + self.ttl,
                },
            );
        }
        Ok(price)
    }
}

fn trim_price_cache_for_insert(
    entries: &DashMap<PriceCacheKey, CachedPrice>,
    keep: &PriceCacheKey,
    max_entries: usize,
) {
    while max_entries > 0 && entries.len() >= max_entries && !entries.contains_key(keep) {
        let evict = entries.iter().next().map(|entry| entry.key().clone());
        let Some(evict) = evict else {
            break;
        };
        entries.remove(&evict);
    }
}

fn scale_video_price_tiers(
    mut tiers: Vec<VideoPriceTier>,
    multiplier_micros: i64,
) -> Vec<VideoPriceTier> {
    for tier in &mut tiers {
        tier.input_with_video_micros =
            scale_optional_micros(tier.input_with_video_micros, multiplier_micros);
        tier.input_without_video_micros =
            scale_optional_micros(tier.input_without_video_micros, multiplier_micros);
        tier.input_with_video_unit_micros =
            scale_optional_micros(tier.input_with_video_unit_micros, multiplier_micros);
        tier.input_without_video_unit_micros =
            scale_optional_micros(tier.input_without_video_unit_micros, multiplier_micros);
    }
    tiers
}

fn scale_optional_micros(value: Option<i64>, multiplier_micros: i64) -> Option<i64> {
    value.map(|value| {
        let product = (value as i128).saturating_mul(multiplier_micros as i128);
        let rounded = (product + 500_000) / 1_000_000;
        i64::try_from(rounded).unwrap_or(i64::MAX)
    })
}

async fn allocate_user_key(
    tx: &mut Transaction<'_, Postgres>,
    user_key_id: DbId,
    user_id: DbId,
    credit_account: &CreditAccountId,
    target_micros: i64,
) -> AppResult<Option<(DbId, i64)>> {
    let row = sqlx::query(
        "SELECT w.balance_micros, w.reserved_micros
         FROM user_key uk
         JOIN credit_account w ON w.owner_type = 'user_key' AND w.owner_id = uk.id
         WHERE uk.id = $1 AND uk.user_id = $2 AND w.id = $3
         FOR UPDATE OF w",
    )
    .bind(user_key_id)
    .bind(user_id)
    .bind(credit_account.id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let balance: i64 = row.try_get("balance_micros")?;
    let reserved: i64 = row.try_get("reserved_micros")?;
    reserve_available_credit(tx, credit_account, balance, reserved, target_micros).await
}

async fn allocate_user_key_model(
    tx: &mut Transaction<'_, Postgres>,
    user_key_id: DbId,
    user_id: DbId,
    credit_account: &CreditAccountId,
    target_micros: i64,
) -> AppResult<Option<(DbId, i64)>> {
    let row = sqlx::query(
        "SELECT w.balance_micros, w.reserved_micros
         FROM user_key_model ukm
         JOIN user_key uk ON uk.id = ukm.user_key_id
         JOIN credit_account w ON w.owner_type = 'user_key_model' AND w.owner_id = ukm.id
         WHERE ukm.user_key_id = $1
           AND uk.user_id = $2
           AND w.id = $3
           AND ukm.enabled = TRUE
         FOR UPDATE OF w",
    )
    .bind(user_key_id)
    .bind(user_id)
    .bind(credit_account.id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let balance: i64 = row.try_get("balance_micros")?;
    let reserved: i64 = row.try_get("reserved_micros")?;
    reserve_available_credit(tx, credit_account, balance, reserved, target_micros).await
}

async fn allocate_project(
    tx: &mut Transaction<'_, Postgres>,
    project_id: DbId,
    credit_account: &CreditAccountId,
    target_micros: i64,
) -> AppResult<Option<(DbId, i64)>> {
    let row = sqlx::query(
        r#"SELECT w.balance_micros, w.reserved_micros
           FROM project p
           JOIN credit_account w ON w.owner_type = 'project' AND w.owner_id = p.id
           WHERE p.id = $1 AND w.id = $2
           FOR UPDATE OF w"#,
    )
    .bind(project_id)
    .bind(credit_account.id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let balance: i64 = row.try_get("balance_micros")?;
    let reserved: i64 = row.try_get("reserved_micros")?;
    reserve_available_credit(tx, credit_account, balance, reserved, target_micros).await
}

async fn reserve_available_credit(
    tx: &mut Transaction<'_, Postgres>,
    credit_account: &CreditAccountId,
    balance: i64,
    reserved: i64,
    target_micros: i64,
) -> AppResult<Option<(DbId, i64)>> {
    let available = (balance - reserved).max(0);
    let amount = target_micros.min(available);
    if amount <= 0 {
        return Ok(None);
    }

    sqlx::query(
        "UPDATE credit_account
         SET reserved_micros = reserved_micros + $2,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(credit_account.id)
    .bind(amount)
    .execute(&mut **tx)
    .await?;
    let row = sqlx::query(
        "INSERT INTO credit_allocation (credit_account_id, amount_micros)
         VALUES ($1, $2)
         RETURNING id",
    )
    .bind(credit_account.id)
    .bind(amount)
    .fetch_one(&mut **tx)
    .await?;
    Ok(Some((row.try_get("id")?, amount)))
}

async fn fetch_stale_allocations(
    pool: &PgPool,
    stale_before: DateTime<Utc>,
) -> AppResult<Vec<HotAllocation>> {
    let rows = sqlx::query(
        "SELECT id, credit_account_id
         FROM credit_allocation
         WHERE status = 'active'
           AND created_at < $1
           AND consumed_micros + returned_micros < amount_micros
           AND NOT EXISTS (
               SELECT 1
               FROM billing b
               CROSS JOIN LATERAL jsonb_array_elements(
                   COALESCE(b.payload->'billing'->'parts', '[]'::jsonb) ||
                   COALESCE(b.payload->'billing'->'returned_parts', '[]'::jsonb)
               ) part
               WHERE b.status IN ('pending', 'failed')
                 AND part->>'allocation_id' = credit_allocation.id::TEXT
           )
           AND NOT EXISTS (
               SELECT 1
               FROM task_upstream task
               WHERE task.billing_status = 'held'
                 AND task.billing_hold @> jsonb_build_object(
                     'parts',
                     jsonb_build_array(
                         jsonb_build_object('allocation_id', credit_allocation.id)
                     )
                 )
           )
         ORDER BY id ASC
         LIMIT 500",
    )
    .bind(stale_before)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(HotAllocation {
                credit_account: CreditAccountId::new(row.try_get("credit_account_id")?),
                allocation_id: row.try_get("id")?,
            })
        })
        .collect()
}

async fn recover_allocations_in_db(
    pool: &PgPool,
    allocations: &[HotAllocation],
) -> AppResult<AllocationRecoverySummary> {
    let ids = allocations
        .iter()
        .map(|allocation| allocation.allocation_id)
        .collect::<Vec<_>>();
    let mut tx = pool.begin().await?;
    let rows = sqlx::query(
        "SELECT id, credit_account_id, amount_micros, consumed_micros, returned_micros, created_at
         FROM credit_allocation
         WHERE id = ANY($1) AND status = 'active'
         ORDER BY id ASC
         FOR UPDATE",
    )
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await?;

    let now = Utc::now();
    let mut summary = AllocationRecoverySummary::default();
    for row in rows {
        let allocation_id: DbId = row.try_get("id")?;
        let credit_account_id: DbId = row.try_get("credit_account_id")?;
        let amount: i64 = row.try_get("amount_micros")?;
        let consumed: i64 = row.try_get("consumed_micros")?;
        let returned: i64 = row.try_get("returned_micros")?;
        let created_at: DateTime<Utc> = row.try_get("created_at")?;
        let recover_amount = amount - consumed - returned;
        if recover_amount <= 0 {
            continue;
        }
        let age_seconds = (now - created_at).num_seconds().max(0);

        let credit_account = CreditAccountId::new(credit_account_id);
        let balance_after =
            account::decrement_reserved_returning_balance(&mut tx, &credit_account, recover_amount)
                .await?;

        account::mark_allocation_recovered(&mut tx, allocation_id).await?;

        sqlx::query(
            "INSERT INTO credit_ledger
             (credit_account_id, amount_micros, balance_after_micros, reason,
              allocation_id, transaction_id, metadata)
             VALUES ($1, 0, $2, 'allocation_recover', $3, $4, $5)",
        )
        .bind(credit_account_id)
        .bind(balance_after)
        .bind(allocation_id)
        .bind(Uuid::new_v4())
        .bind(serde_json::json!({
            "returned_reserved_micros": recover_amount
        }))
        .execute(&mut *tx)
        .await?;
        summary.count += 1;
        summary.recovered_micros += recover_amount;
        if summary
            .oldest_created_at
            .is_none_or(|oldest| created_at < oldest)
        {
            summary.oldest_created_at = Some(created_at);
            summary.oldest_age_seconds = Some(age_seconds);
        }
        if summary.samples.len() < ALLOCATION_RECOVERY_LOG_SAMPLE_LIMIT {
            summary.samples.push(AllocationRecoverySample {
                allocation_id,
                credit_account_id,
                amount_micros: amount,
                consumed_micros: consumed,
                already_returned_micros: returned,
                recovered_micros: recover_amount,
                age_seconds,
            });
        } else {
            summary.truncated += 1;
        }
    }
    tx.commit().await?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sqlx::PgPool;

    use super::*;

    fn credit_account(id: DbId) -> CreditAccountId {
        CreditAccountId::new(id)
    }

    fn billing_accounts<'a>(
        user_key_credit_account: &'a CreditAccountId,
        project_credit_account: &'a CreditAccountId,
    ) -> BillingAccounts<'a> {
        BillingAccounts {
            user_id: 1,
            project_id: 1,
            user_key_id: 1,
            user_key_model_credit_account: None,
            user_key_credit_account,
            project_credit_account,
        }
    }

    fn token_price() -> Price {
        Price {
            input_price_micros: MICROS_PER_MAJOR_UNIT,
            output_price_micros: MICROS_PER_MAJOR_UNIT,
            cache_read_price_micros: None,
            cache_write_price_micros: None,
            billing_meter: BillingMeter::Token,
            unit_price_micros: None,
            video_billing_mode: None,
            video_price_tiers: Vec::new(),
        }
    }

    #[tokio::test]
    async fn settle_keeps_unused_reserved_credit_out_of_hot_store_until_recorded() {
        let billing = Billing::new_memory(Duration::from_secs(60), 32, 100, 100);
        let pool = PgPool::connect_lazy("postgres://neogate:neogate@localhost/neogate").unwrap();
        let user_key_credit_account = credit_account(10);
        let project_credit_account = credit_account(20);

        billing
            .hot
            .credit_allocation(user_key_credit_account.clone(), 101, 100)
            .await
            .unwrap();

        let hold = billing
            .reserve(
                &pool,
                billing_accounts(&user_key_credit_account, &project_credit_account),
                100,
            )
            .await
            .unwrap();
        assert_eq!(
            hold.parts
                .iter()
                .map(|part| part.amount_micros)
                .sum::<i64>(),
            100
        );

        let charge = billing
            .settle(
                &pool,
                SettleRequest {
                    accounts: billing_accounts(&user_key_credit_account, &project_credit_account),
                    hold,
                    usage: Some(BillableUsage::token(TokenUsage {
                        input_tokens: 40,
                        output_tokens: 0,
                        cached_input_tokens: None,
                        cache_creation_input_tokens: None,
                        cache_creation_input_tokens_5m: None,
                        cache_creation_input_tokens_1h: None,
                        reasoning_output_tokens: None,
                        audio_input_tokens: None,
                        audio_output_tokens: None,
                    })),
                    price: &token_price(),
                },
            )
            .await
            .unwrap();

        assert_eq!(
            charge
                .parts
                .iter()
                .map(|part| part.amount_micros)
                .sum::<i64>(),
            40
        );
        assert_eq!(
            charge
                .returned_parts
                .iter()
                .map(|part| part.amount_micros)
                .sum::<i64>(),
            60
        );
        assert!(billing
            .hot
            .try_debit_ordered(&[user_key_credit_account], 1)
            .await
            .unwrap()
            .is_none());
    }
}
