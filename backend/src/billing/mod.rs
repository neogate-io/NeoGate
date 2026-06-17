use std::{
    collections::hash_map::DefaultHasher,
    collections::HashMap,
    fmt,
    hash::{Hash, Hasher},
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
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

use credit::{HotAllocation, HotCreditStore, MemoryHotCreditStore, RedisHotCreditStore};
pub use metering::{
    cost_for_usage, estimate_input_tokens, estimated_cost_micro_usd, parse_usage_from_bytes,
    parse_usage_from_sse_data,
};
pub use types::{
    BillingCharge, CreditAccountId, CreditAccountType, DebitHold, DebitPart, Price, TokenUsage,
};

pub const MICRO_USD_PER_USD: i64 = 1_000_000;
const ALLOCATION_RECOVERY_LOG_SAMPLE_LIMIT: usize = 20;

#[derive(Clone)]
pub struct Billing {
    hot: Arc<dyn HotCreditStore>,
    price_cache: PriceCache,
    prefetch_locks: Arc<Vec<Mutex<()>>>,
    prefetch_micro_usd: i64,
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
    pub usage: Option<TokenUsage>,
    pub price: &'a Price,
}

#[derive(Debug, Default)]
struct AllocationRecoverySummary {
    count: u64,
    recovered_micro_usd: i64,
    oldest_created_at: Option<DateTime<Utc>>,
    oldest_age_seconds: Option<i64>,
    samples: Vec<AllocationRecoverySample>,
    truncated: usize,
}

struct AllocationRecoverySample {
    allocation_id: DbId,
    credit_account_id: DbId,
    amount_micro_usd: i64,
    consumed_micro_usd: i64,
    already_returned_micro_usd: i64,
    recovered_micro_usd: i64,
    age_seconds: i64,
}

impl fmt::Debug for AllocationRecoverySample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AllocationRecoverySample")
            .field("allocation_id", &self.allocation_id)
            .field("credit_account_id", &self.credit_account_id)
            .field("amount_micro_usd", &self.amount_micro_usd)
            .field("consumed_micro_usd", &self.consumed_micro_usd)
            .field(
                "already_returned_micro_usd",
                &self.already_returned_micro_usd,
            )
            .field("recovered_micro_usd", &self.recovered_micro_usd)
            .field("age_seconds", &self.age_seconds)
            .finish()
    }
}

type PriceCacheKey = (String, String, String);
type PriceCacheEntries = HashMap<PriceCacheKey, CachedPrice>;

#[derive(Clone)]
struct PriceCache {
    ttl: Duration,
    max_entries: usize,
    entries: Arc<RwLock<PriceCacheEntries>>,
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
        prefetch_micro_usd: i64,
        default_output_tokens: i64,
    ) -> Self {
        Self {
            hot: Arc::new(MemoryHotCreditStore::default()),
            price_cache: PriceCache::new(price_cache_ttl, price_cache_max_entries),
            prefetch_locks: prefetch_locks(),
            prefetch_micro_usd,
            default_output_tokens,
        }
    }

    pub async fn new_redis(
        redis_url: &str,
        key_prefix: String,
        price_cache_ttl: Duration,
        price_cache_max_entries: usize,
        prefetch_micro_usd: i64,
        default_output_tokens: i64,
    ) -> AppResult<Self> {
        Ok(Self {
            hot: Arc::new(RedisHotCreditStore::connect(redis_url, key_prefix).await?),
            price_cache: PriceCache::new(price_cache_ttl, price_cache_max_entries),
            prefetch_locks: prefetch_locks(),
            prefetch_micro_usd,
            default_output_tokens,
        })
    }

    pub fn default_output_tokens(&self) -> i64 {
        self.default_output_tokens
    }

    pub async fn price_for(
        &self,
        pool: &PgPool,
        provider: &str,
        model: &str,
        user_group: &str,
    ) -> AppResult<Price> {
        self.price_cache
            .price_for(pool, provider, model, user_group)
            .await
    }

    pub fn invalidate_price(&self, provider: &str, model: &str) {
        self.price_cache.invalidate(provider, model);
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
                            recovered_micro_usd = summary.recovered_micro_usd,
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
        estimated_micro_usd: i64,
    ) -> AppResult<DebitHold> {
        if estimated_micro_usd <= 0 {
            return Ok(DebitHold {
                transaction_id: Uuid::new_v4(),
                estimated_micro_usd: 0,
                parts: Vec::new(),
                charge_credit: true,
            });
        }

        let credit_accounts = ordered_credit_accounts(accounts);
        if let Some(parts) = self
            .hot
            .try_debit_ordered(&credit_accounts, estimated_micro_usd)
            .await?
        {
            return Ok(DebitHold {
                transaction_id: Uuid::new_v4(),
                estimated_micro_usd,
                parts,
                charge_credit: true,
            });
        }

        let lock_id = prefetch_lock_index(accounts, self.prefetch_locks.len());
        let _prefetch_guard = self.prefetch_locks[lock_id].lock().await;

        if let Some(parts) = self
            .hot
            .try_debit_ordered(&credit_accounts, estimated_micro_usd)
            .await?
        {
            return Ok(DebitHold {
                transaction_id: Uuid::new_v4(),
                estimated_micro_usd,
                parts,
                charge_credit: true,
            });
        }

        self.prefetch(pool, accounts, estimated_micro_usd).await?;

        let Some(parts) = self
            .hot
            .try_debit_ordered(&credit_accounts, estimated_micro_usd)
            .await?
        else {
            return Err(AppError::PaymentRequired);
        };

        Ok(DebitHold {
            transaction_id: Uuid::new_v4(),
            estimated_micro_usd,
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
            if part.amount_micro_usd <= 0 {
                continue;
            }
            account::decrement_reserved(&mut tx, &part.credit_account, part.amount_micro_usd)
                .await?;
            account::mark_allocation_returned(&mut tx, part.allocation_id, part.amount_micro_usd)
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
        let (cost_micro_usd, status) = match usage {
            Some(usage) => (cost_for_usage(usage, price), "billed".to_string()),
            None => (hold.estimated_micro_usd, "usage_missing".to_string()),
        };
        let cost_micro_usd = cost_micro_usd.max(0);

        if !hold.charge_credit {
            return Ok(BillingCharge {
                transaction_id: hold.transaction_id,
                input_tokens: usage.map(|usage| usage.input_tokens),
                output_tokens: usage.map(|usage| usage.output_tokens),
                total_tokens: usage.map(TokenUsage::total_tokens),
                cost_micro_usd,
                status,
                parts: Vec::new(),
                returned_parts: Vec::new(),
            });
        }

        if cost_micro_usd >= hold.estimated_micro_usd {
            if cost_micro_usd > hold.estimated_micro_usd {
                let supplemental_micro_usd = cost_micro_usd - hold.estimated_micro_usd;
                match self.reserve(pool, accounts, supplemental_micro_usd).await {
                    Ok(extra_hold) => {
                        let mut parts = hold.parts;
                        parts.extend(extra_hold.parts);
                        return Ok(BillingCharge {
                            transaction_id: hold.transaction_id,
                            input_tokens: usage.map(|usage| usage.input_tokens),
                            output_tokens: usage.map(|usage| usage.output_tokens),
                            total_tokens: usage.map(TokenUsage::total_tokens),
                            cost_micro_usd,
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
                input_tokens: usage.map(|usage| usage.input_tokens),
                output_tokens: usage.map(|usage| usage.output_tokens),
                total_tokens: usage.map(TokenUsage::total_tokens),
                cost_micro_usd: hold.estimated_micro_usd,
                status: if cost_micro_usd > hold.estimated_micro_usd {
                    "undercharged".to_string()
                } else {
                    status
                },
                parts: hold.parts,
                returned_parts: Vec::new(),
            });
        }

        let mut remaining = cost_micro_usd;
        let mut consumed = Vec::new();
        let mut refund = Vec::new();
        for part in hold.parts {
            if remaining <= 0 {
                refund.push(part);
                continue;
            }
            let consume = part.amount_micro_usd.min(remaining);
            remaining -= consume;
            consumed.push(DebitPart {
                amount_micro_usd: consume,
                ..part.clone()
            });
            if part.amount_micro_usd > consume {
                refund.push(DebitPart {
                    amount_micro_usd: part.amount_micro_usd - consume,
                    ..part
                });
            }
        }
        let returned_parts = match self.hot.refund(&refund).await {
            Ok(returned_parts) => returned_parts,
            Err(err) => {
                tracing::warn!(
                    "failed to refund unused hot credit; reserved credit will be recovered later: {err}"
                );
                Vec::new()
            }
        };

        Ok(BillingCharge {
            transaction_id: hold.transaction_id,
            input_tokens: usage.map(|usage| usage.input_tokens),
            output_tokens: usage.map(|usage| usage.output_tokens),
            total_tokens: usage.map(TokenUsage::total_tokens),
            cost_micro_usd,
            status,
            parts: consumed,
            returned_parts,
        })
    }

    async fn prefetch(
        &self,
        pool: &PgPool,
        accounts: BillingAccounts<'_>,
        needed_micro_usd: i64,
    ) -> AppResult<()> {
        let mut tx = pool.begin().await?;
        let mut remaining = needed_micro_usd;
        let target = self.prefetch_micro_usd.max(needed_micro_usd);
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
            let key_target = self.prefetch_micro_usd.max(remaining);
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
            let project_target = self.prefetch_micro_usd.max(remaining);
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
        credit_accounts.push(credit_account.clone());
    }
    credit_accounts.push(accounts.user_key_credit_account.clone());
    credit_accounts.push(accounts.project_credit_account.clone());
    credit_accounts
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
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn invalidate(&self, provider: &str, model: &str) {
        self.entries.write().expect("price cache poisoned").retain(
            |(cached_provider, cached_model, _), _| {
                cached_provider != provider || cached_model != model
            },
        );
    }

    fn invalidate_all(&self) {
        self.entries.write().expect("price cache poisoned").clear();
    }

    async fn price_for(
        &self,
        pool: &PgPool,
        provider: &str,
        model: &str,
        user_group: &str,
    ) -> AppResult<Price> {
        let key = (
            provider.to_string(),
            model.to_string(),
            user_group.to_string(),
        );
        {
            let entries = self.entries.read().expect("price cache poisoned");
            if let Some(cached) = entries.get(&key) {
                if cached.expires_at > Instant::now() {
                    return Ok(cached.price.clone());
                }
            }
        }

        let row = sqlx::query(
            r#"
            WITH base_price AS (
                SELECT input_price_usd_micros, output_price_usd_micros,
                       cache_read_price_usd_micros,
                       cache_write_price_usd_micros
                FROM provider_price
                WHERE provider = $1 AND model = $2 AND enabled = TRUE
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
                (base_price.input_price_usd_micros *
                    COALESCE(policy.multiplier_micros, 1000000) + 500000) / 1000000
                    AS input_price_usd_micros,
                (base_price.output_price_usd_micros *
                    COALESCE(policy.multiplier_micros, 1000000) + 500000) / 1000000
                    AS output_price_usd_micros,
                CASE
                    WHEN base_price.cache_read_price_usd_micros IS NULL THEN NULL
                    ELSE (base_price.cache_read_price_usd_micros *
                        COALESCE(policy.multiplier_micros, 1000000) + 500000) / 1000000
                END AS cache_read_price_usd_micros,
                CASE
                    WHEN base_price.cache_write_price_usd_micros IS NULL THEN NULL
                    ELSE (base_price.cache_write_price_usd_micros *
                        COALESCE(policy.multiplier_micros, 1000000) + 500000) / 1000000
                END AS cache_write_price_usd_micros
            FROM base_price
            LEFT JOIN policy ON TRUE
            "#,
        )
        .bind(provider)
        .bind(model)
        .bind(user_group)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| {
            AppError::BadRequest(format!("price is not configured for {provider}/{model}"))
        })?;

        let price = Price {
            input_price_usd_micros: row.try_get("input_price_usd_micros")?,
            output_price_usd_micros: row.try_get("output_price_usd_micros")?,
            cache_read_price_usd_micros: row.try_get("cache_read_price_usd_micros")?,
            cache_write_price_usd_micros: row.try_get("cache_write_price_usd_micros")?,
        };
        if !self.ttl.is_zero() {
            let now = Instant::now();
            let mut entries = self.entries.write().expect("price cache poisoned");
            entries.retain(|_, cached| cached.expires_at > now);
            trim_price_cache_for_insert(&mut entries, &key, self.max_entries);
            entries.insert(
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
    entries: &mut PriceCacheEntries,
    keep: &PriceCacheKey,
    max_entries: usize,
) {
    while max_entries > 0 && entries.len() >= max_entries && !entries.contains_key(keep) {
        let Some(evict) = entries.keys().next().cloned() else {
            break;
        };
        entries.remove(&evict);
    }
}

async fn allocate_user_key(
    tx: &mut Transaction<'_, Postgres>,
    user_key_id: DbId,
    user_id: DbId,
    credit_account: &CreditAccountId,
    target_micro_usd: i64,
) -> AppResult<Option<(DbId, i64)>> {
    let row = sqlx::query(
        "SELECT w.balance_micro_usd, w.reserved_micro_usd
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
    let balance: i64 = row.try_get("balance_micro_usd")?;
    let reserved: i64 = row.try_get("reserved_micro_usd")?;
    reserve_available_credit(tx, credit_account, balance, reserved, target_micro_usd).await
}

async fn allocate_user_key_model(
    tx: &mut Transaction<'_, Postgres>,
    user_key_id: DbId,
    user_id: DbId,
    credit_account: &CreditAccountId,
    target_micro_usd: i64,
) -> AppResult<Option<(DbId, i64)>> {
    let row = sqlx::query(
        "SELECT w.balance_micro_usd, w.reserved_micro_usd
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
    let balance: i64 = row.try_get("balance_micro_usd")?;
    let reserved: i64 = row.try_get("reserved_micro_usd")?;
    reserve_available_credit(tx, credit_account, balance, reserved, target_micro_usd).await
}

async fn allocate_project(
    tx: &mut Transaction<'_, Postgres>,
    project_id: DbId,
    credit_account: &CreditAccountId,
    target_micro_usd: i64,
) -> AppResult<Option<(DbId, i64)>> {
    let row = sqlx::query(
        r#"SELECT w.balance_micro_usd, w.reserved_micro_usd
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
    let balance: i64 = row.try_get("balance_micro_usd")?;
    let reserved: i64 = row.try_get("reserved_micro_usd")?;
    reserve_available_credit(tx, credit_account, balance, reserved, target_micro_usd).await
}

async fn reserve_available_credit(
    tx: &mut Transaction<'_, Postgres>,
    credit_account: &CreditAccountId,
    balance: i64,
    reserved: i64,
    target_micro_usd: i64,
) -> AppResult<Option<(DbId, i64)>> {
    let available = (balance - reserved).max(0);
    let amount = target_micro_usd.min(available);
    if amount <= 0 {
        return Ok(None);
    }

    sqlx::query(
        "UPDATE credit_account
         SET reserved_micro_usd = reserved_micro_usd + $2,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(credit_account.id)
    .bind(amount)
    .execute(&mut **tx)
    .await?;
    let row = sqlx::query(
        "INSERT INTO credit_allocation (credit_account_id, amount_micro_usd)
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
           AND consumed_micro_usd + returned_micro_usd < amount_micro_usd
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
        "SELECT id, credit_account_id, amount_micro_usd, consumed_micro_usd, returned_micro_usd, created_at
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
        let amount: i64 = row.try_get("amount_micro_usd")?;
        let consumed: i64 = row.try_get("consumed_micro_usd")?;
        let returned: i64 = row.try_get("returned_micro_usd")?;
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
             (credit_account_id, amount_micro_usd, balance_after_micro_usd, reason,
              allocation_id, transaction_id, metadata)
             VALUES ($1, 0, $2, 'allocation_recover', $3, $4, $5)",
        )
        .bind(credit_account_id)
        .bind(balance_after)
        .bind(allocation_id)
        .bind(Uuid::new_v4())
        .bind(serde_json::json!({
            "returned_reserved_micro_usd": recover_amount
        }))
        .execute(&mut *tx)
        .await?;
        summary.count += 1;
        summary.recovered_micro_usd += recover_amount;
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
                amount_micro_usd: amount,
                consumed_micro_usd: consumed,
                already_returned_micro_usd: returned,
                recovered_micro_usd: recover_amount,
                age_seconds,
            });
        } else {
            summary.truncated += 1;
        }
    }
    tx.commit().await?;
    Ok(summary)
}
