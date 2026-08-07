use std::{
    collections::{hash_map::DefaultHasher, HashMap, HashSet, VecDeque},
    hash::{Hash, Hasher},
    sync::LazyLock,
};

use async_trait::async_trait;
use tokio::sync::{Mutex, MutexGuard};

use crate::{
    error::{AppError, AppResult},
    id::DbId,
};

use super::{CreditAccountId, DebitPart};

#[async_trait]
pub(super) trait HotCreditStore: Send + Sync {
    async fn credit_allocation(
        &self,
        credit_account: CreditAccountId,
        allocation_id: DbId,
        amount_micros: i64,
    ) -> AppResult<()>;
    async fn drain_credit_account(
        &self,
        credit_account: &CreditAccountId,
    ) -> AppResult<Vec<DebitPart>>;
    async fn try_debit_ordered(
        &self,
        credit_accounts: &[CreditAccountId],
        amount_micros: i64,
    ) -> AppResult<Option<Vec<DebitPart>>>;
    async fn available_micros(&self, credit_accounts: &[CreditAccountId]) -> AppResult<i64>;
    async fn remove_allocations(&self, allocations: &[HotAllocation]) -> AppResult<()>;
}

#[derive(Debug, Clone)]
pub(super) struct HotAllocation {
    pub credit_account: CreditAccountId,
    pub allocation_id: DbId,
}

pub(super) struct MemoryHotCreditStore {
    shards: Vec<Mutex<HashMap<CreditAccountId, CreditAccountHotCredit>>>,
}

pub(super) struct RedisHotCreditStore {
    manager: redis::aio::ConnectionManager,
    key_prefix: String,
}

#[derive(Debug, Clone, Default)]
struct CreditAccountHotCredit {
    total_available_micros: i64,
    segments: VecDeque<HotSegment>,
}

#[derive(Debug, Clone)]
struct HotSegment {
    allocation_id: DbId,
    available_micros: i64,
}

const HOT_CREDIT_SHARDS: usize = 64;

// 线格式：列表项为 `allocation_id:segment_amount`。匹配模式统一用不带尾冒号的
// `([^:]+):([^:]+)`，可同时解析历史遗留的三段格式（`id:amount:generation`）与
// 当前两段格式，保证滚动部署期间新旧数据互通。
static REDIS_CREDIT_ALLOCATION_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
    redis::Script::new(
        r#"
        if not redis.call('GET', KEYS[2]) then
          local total = 0
          local items = redis.call('LRANGE', KEYS[1], 0, -1)
          for _, item in ipairs(items) do
            local allocation_id, segment_amount = string.match(item, '([^:]+):([^:]+)')
            total = total + tonumber(segment_amount)
          end
          redis.call('SET', KEYS[2], total)
        end
        redis.call('RPUSH', KEYS[1], ARGV[1] .. ':' .. ARGV[2])
        redis.call('INCRBY', KEYS[2], ARGV[2])
        return 1
        "#,
    )
});

static REDIS_DRAIN_CREDIT_ACCOUNT_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
    redis::Script::new(
        r#"
        local items = redis.call('LRANGE', KEYS[1], 0, -1)
        redis.call('DEL', KEYS[1])
        redis.call('DEL', KEYS[2])
        return items
        "#,
    )
});

static REDIS_DEBIT_ORDERED_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
    redis::Script::new(
        r#"
        local account_count = tonumber(ARGV[1])
        local amount = tonumber(ARGV[2])
        local available = 0
        for i = 1, account_count do
          local list_key = KEYS[i]
          local total_key = KEYS[account_count + i]
          local total = redis.call('GET', total_key)
          if not total then
            local rebuilt = 0
            local items = redis.call('LRANGE', list_key, 0, -1)
            for _, item in ipairs(items) do
              local allocation_id, segment_amount = string.match(item, '([^:]+):([^:]+)')
              rebuilt = rebuilt + tonumber(segment_amount)
            end
            redis.call('SET', total_key, rebuilt)
            total = tostring(rebuilt)
          end
          available = available + tonumber(total)
        end
        if available < amount then
          return {}
        end

        local remaining = amount
        local output = {}
        for i = 1, account_count do
          if remaining <= 0 then break end
          local list_key = KEYS[i]
          local total_key = KEYS[account_count + i]
          while remaining > 0 do
            local item = redis.call('LPOP', list_key)
            if not item then break end
            local allocation_id, segment_amount = string.match(item, '([^:]+):([^:]+)')
            local segment_amount_num = tonumber(segment_amount)
            local debit = segment_amount_num
            if debit > remaining then
              debit = remaining
            end
            remaining = remaining - debit
            redis.call('DECRBY', total_key, debit)
            table.insert(output, tostring(i))
            table.insert(output, allocation_id)
            table.insert(output, tostring(debit))
            local leftover = segment_amount_num - debit
            if leftover > 0 then
              redis.call('LPUSH', list_key, allocation_id .. ':' .. tostring(leftover))
            end
          end
        end
        return output
        "#,
    )
});

static REDIS_AVAILABLE_CREDIT_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
    redis::Script::new(
        r#"
        local account_count = tonumber(ARGV[1])
        local available = 0
        for i = 1, account_count do
          local list_key = KEYS[i]
          local total_key = KEYS[account_count + i]
          local total = redis.call('GET', total_key)
          if not total then
            local rebuilt = 0
            local items = redis.call('LRANGE', list_key, 0, -1)
            for _, item in ipairs(items) do
              local allocation_id, segment_amount = string.match(item, '([^:]+):([^:]+)')
              rebuilt = rebuilt + tonumber(segment_amount)
            end
            redis.call('SET', total_key, rebuilt)
            total = tostring(rebuilt)
          end
          available = available + tonumber(total)
        end
        return available
        "#,
    )
});

static REDIS_REMOVE_ALLOCATIONS_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
    redis::Script::new(
        r#"
        local remove = {}
        for i = 1, #ARGV do
          remove[ARGV[i]] = true
        end

        local items = redis.call('LRANGE', KEYS[1], 0, -1)
        redis.call('DEL', KEYS[1])
        local kept = 0
        for _, item in ipairs(items) do
            local allocation_id, segment_amount = string.match(item, '([^:]+):([^:]+)')
            if not remove[allocation_id] then
                redis.call('RPUSH', KEYS[1], item)
                kept = kept + tonumber(segment_amount)
            end
        end
        if kept > 0 then
            redis.call('SET', KEYS[2], kept)
        else
            redis.call('DEL', KEYS[2])
        end
        return 1
        "#,
    )
});

#[async_trait]
impl HotCreditStore for MemoryHotCreditStore {
    async fn credit_allocation(
        &self,
        credit_account: CreditAccountId,
        allocation_id: DbId,
        amount_micros: i64,
    ) -> AppResult<()> {
        if amount_micros <= 0 {
            return Ok(());
        }
        let mut balances = self.lock_shard_for_credit_account(&credit_account).await;
        let account_hot = balances.entry(credit_account).or_default();
        account_hot.total_available_micros += amount_micros;
        account_hot.segments.push_back(HotSegment {
            allocation_id,
            available_micros: amount_micros,
        });
        Ok(())
    }

    async fn drain_credit_account(
        &self,
        credit_account: &CreditAccountId,
    ) -> AppResult<Vec<DebitPart>> {
        let mut balances = self.lock_shard_for_credit_account(credit_account).await;
        let account_hot = balances.entry(credit_account.clone()).or_default();
        account_hot.total_available_micros = 0;
        Ok(std::mem::take(&mut account_hot.segments)
            .into_iter()
            .filter(|segment| segment.available_micros > 0)
            .map(|segment| DebitPart {
                credit_account: credit_account.clone(),
                allocation_id: segment.allocation_id,
                amount_micros: segment.available_micros,
            })
            .collect())
    }

    async fn try_debit_ordered(
        &self,
        credit_accounts: &[CreditAccountId],
        amount_micros: i64,
    ) -> AppResult<Option<Vec<DebitPart>>> {
        if amount_micros <= 0 {
            return Ok(Some(Vec::new()));
        }

        let mut shard_ids = credit_accounts
            .iter()
            .map(Self::shard_index)
            .collect::<Vec<_>>();
        shard_ids.sort_unstable();
        shard_ids.dedup();
        let mut shards = Vec::with_capacity(shard_ids.len());
        for id in shard_ids {
            shards.push((id, self.shards[id].lock().await));
        }

        let available = credit_accounts
            .iter()
            .filter_map(|credit_account| {
                let shard_id = Self::shard_index(credit_account);
                shards
                    .iter()
                    .find(|(id, _)| *id == shard_id)
                    .and_then(|(_, balances)| balances.get(credit_account))
            })
            .map(|account_hot| account_hot.total_available_micros)
            .sum::<i64>();
        if available < amount_micros {
            return Ok(None);
        }

        let mut remaining = amount_micros;
        let mut parts = Vec::new();
        for credit_account in credit_accounts {
            if remaining <= 0 {
                break;
            }
            let shard_id = Self::shard_index(credit_account);
            let Some((_, balances)) = shards.iter_mut().find(|(id, _)| *id == shard_id) else {
                continue;
            };
            let Some(account_hot) = balances.get_mut(credit_account) else {
                continue;
            };
            let segments = &mut account_hot.segments;
            while remaining > 0 {
                let Some(front) = segments.front_mut() else {
                    break;
                };
                let debit = front.available_micros.min(remaining);
                front.available_micros -= debit;
                account_hot.total_available_micros -= debit;
                remaining -= debit;
                parts.push(DebitPart {
                    credit_account: credit_account.clone(),
                    allocation_id: front.allocation_id,
                    amount_micros: debit,
                });
                if front.available_micros == 0 {
                    segments.pop_front();
                }
            }
        }
        if remaining > 0 {
            for part in parts.iter().rev() {
                let Some((_, balances)) = shards
                    .iter_mut()
                    .find(|(id, _)| *id == Self::shard_index(&part.credit_account))
                else {
                    continue;
                };
                let account_hot = balances.entry(part.credit_account.clone()).or_default();
                account_hot.total_available_micros += part.amount_micros;
                account_hot.segments.push_front(HotSegment {
                    allocation_id: part.allocation_id,
                    available_micros: part.amount_micros,
                });
            }
            return Ok(None);
        }
        Ok(Some(parts))
    }

    async fn available_micros(&self, credit_accounts: &[CreditAccountId]) -> AppResult<i64> {
        let mut shard_ids = credit_accounts
            .iter()
            .map(Self::shard_index)
            .collect::<Vec<_>>();
        shard_ids.sort_unstable();
        shard_ids.dedup();
        let mut shards = Vec::with_capacity(shard_ids.len());
        for id in shard_ids {
            shards.push((id, self.shards[id].lock().await));
        }

        Ok(credit_accounts
            .iter()
            .filter_map(|credit_account| {
                let shard_id = Self::shard_index(credit_account);
                shards
                    .iter()
                    .find(|(id, _)| *id == shard_id)
                    .and_then(|(_, balances)| balances.get(credit_account))
            })
            .map(|account_hot| account_hot.total_available_micros)
            .sum())
    }

    async fn remove_allocations(&self, allocations: &[HotAllocation]) -> AppResult<()> {
        if allocations.is_empty() {
            return Ok(());
        }
        let mut by_credit_account: HashMap<CreditAccountId, HashSet<DbId>> = HashMap::new();
        for allocation in allocations {
            by_credit_account
                .entry(allocation.credit_account.clone())
                .or_default()
                .insert(allocation.allocation_id);
        }

        for (credit_account, allocation_ids) in by_credit_account {
            let mut balances = self.lock_shard_for_credit_account(&credit_account).await;
            let Some(account_hot) = balances.get_mut(&credit_account) else {
                continue;
            };
            let mut kept_total = 0;
            account_hot.segments.retain(|segment| {
                if allocation_ids.contains(&segment.allocation_id) {
                    false
                } else {
                    kept_total += segment.available_micros;
                    true
                }
            });
            account_hot.total_available_micros = kept_total;
        }
        Ok(())
    }
}

impl Default for MemoryHotCreditStore {
    fn default() -> Self {
        Self {
            shards: (0..HOT_CREDIT_SHARDS)
                .map(|_| Mutex::new(HashMap::new()))
                .collect(),
        }
    }
}

impl MemoryHotCreditStore {
    fn shard_index(credit_account: &CreditAccountId) -> usize {
        let mut hasher = DefaultHasher::new();
        credit_account.hash(&mut hasher);
        hasher.finish() as usize % HOT_CREDIT_SHARDS
    }

    async fn lock_shard_for_credit_account(
        &self,
        credit_account: &CreditAccountId,
    ) -> MutexGuard<'_, HashMap<CreditAccountId, CreditAccountHotCredit>> {
        self.shards[Self::shard_index(credit_account)].lock().await
    }
}

impl RedisHotCreditStore {
    pub async fn connect(redis_url: &str, key_prefix: String) -> AppResult<Self> {
        let client = redis::Client::open(redis_url)?;
        let manager = client.get_connection_manager().await?;
        Ok(Self {
            manager,
            key_prefix,
        })
    }

    fn credit_account_key(&self, credit_account: &CreditAccountId) -> String {
        format!("{}:hot_credit:{}", self.key_prefix, credit_account.id)
    }

    fn total_key(&self, credit_account: &CreditAccountId) -> String {
        format!("{}:hot_credit_total:{}", self.key_prefix, credit_account.id)
    }
}

#[async_trait]
impl HotCreditStore for RedisHotCreditStore {
    async fn credit_allocation(
        &self,
        credit_account: CreditAccountId,
        allocation_id: DbId,
        amount_micros: i64,
    ) -> AppResult<()> {
        if amount_micros <= 0 {
            return Ok(());
        }
        let credit_account_key = self.credit_account_key(&credit_account);
        let total_key = self.total_key(&credit_account);
        let mut conn = self.manager.clone();
        let _: i64 = REDIS_CREDIT_ALLOCATION_SCRIPT
            .key(credit_account_key)
            .key(total_key)
            .arg(allocation_id)
            .arg(amount_micros)
            .invoke_async(&mut conn)
            .await?;
        Ok(())
    }

    async fn drain_credit_account(
        &self,
        credit_account: &CreditAccountId,
    ) -> AppResult<Vec<DebitPart>> {
        let credit_account_key = self.credit_account_key(credit_account);
        let total_key = self.total_key(credit_account);
        let mut conn = self.manager.clone();
        let items: Vec<String> = REDIS_DRAIN_CREDIT_ACCOUNT_SCRIPT
            .key(credit_account_key)
            .key(total_key)
            .invoke_async(&mut conn)
            .await?;
        Ok(items
            .into_iter()
            .filter_map(|item| decode_hot_segment(credit_account, &item))
            .collect())
    }

    async fn try_debit_ordered(
        &self,
        credit_accounts: &[CreditAccountId],
        amount_micros: i64,
    ) -> AppResult<Option<Vec<DebitPart>>> {
        if amount_micros <= 0 {
            return Ok(Some(Vec::new()));
        }
        if credit_accounts.is_empty() {
            return Ok(None);
        }
        let credit_account_keys = credit_accounts
            .iter()
            .map(|credit_account| self.credit_account_key(credit_account))
            .collect::<Vec<_>>();
        let total_keys = credit_accounts
            .iter()
            .map(|credit_account| self.total_key(credit_account))
            .collect::<Vec<_>>();
        let mut conn = self.manager.clone();
        let mut invocation = REDIS_DEBIT_ORDERED_SCRIPT.prepare_invoke();
        for key in &credit_account_keys {
            invocation.key(key);
        }
        for key in &total_keys {
            invocation.key(key);
        }
        invocation.arg(credit_accounts.len()).arg(amount_micros);
        let values: Vec<String> = invocation.invoke_async(&mut conn).await?;
        if values.is_empty() {
            return Ok(None);
        }
        let mut parts = Vec::new();
        let mut chunks = values.chunks_exact(3);
        for chunk in &mut chunks {
            parts.push(decode_redis_debit_part(credit_accounts, chunk)?);
        }
        if !chunks.remainder().is_empty() {
            return Err(AppError::BadRequest(
                "invalid redis hot credit debit response".to_string(),
            ));
        }
        Ok(Some(parts))
    }

    async fn available_micros(&self, credit_accounts: &[CreditAccountId]) -> AppResult<i64> {
        if credit_accounts.is_empty() {
            return Ok(0);
        }
        let credit_account_keys = credit_accounts
            .iter()
            .map(|credit_account| self.credit_account_key(credit_account))
            .collect::<Vec<_>>();
        let total_keys = credit_accounts
            .iter()
            .map(|credit_account| self.total_key(credit_account))
            .collect::<Vec<_>>();
        let mut conn = self.manager.clone();
        let mut invocation = REDIS_AVAILABLE_CREDIT_SCRIPT.prepare_invoke();
        for key in &credit_account_keys {
            invocation.key(key);
        }
        for key in &total_keys {
            invocation.key(key);
        }
        invocation.arg(credit_accounts.len());
        Ok(invocation.invoke_async(&mut conn).await?)
    }

    async fn remove_allocations(&self, allocations: &[HotAllocation]) -> AppResult<()> {
        if allocations.is_empty() {
            return Ok(());
        }
        let mut by_credit_account: HashMap<CreditAccountId, Vec<DbId>> = HashMap::new();
        for allocation in allocations {
            by_credit_account
                .entry(allocation.credit_account.clone())
                .or_default()
                .push(allocation.allocation_id);
        }

        let mut conn = self.manager.clone();
        for (credit_account, allocation_ids) in by_credit_account {
            let credit_account_key = self.credit_account_key(&credit_account);
            let total_key = self.total_key(&credit_account);
            let mut invocation = REDIS_REMOVE_ALLOCATIONS_SCRIPT.prepare_invoke();
            invocation.key(credit_account_key);
            invocation.key(total_key);
            for allocation_id in allocation_ids {
                invocation.arg(allocation_id);
            }
            let _: i64 = invocation.invoke_async(&mut conn).await?;
        }
        Ok(())
    }
}

fn decode_redis_debit_part(
    credit_accounts: &[CreditAccountId],
    chunk: &[String],
) -> AppResult<DebitPart> {
    let account_position = chunk[0].parse::<usize>().map_err(|err| {
        AppError::BadRequest(format!("invalid redis hot credit account index: {err}"))
    })?;
    let Some(account_index) = account_position.checked_sub(1) else {
        return Err(AppError::BadRequest(
            "invalid redis hot credit account index: 0".to_string(),
        ));
    };
    let credit_account = credit_accounts.get(account_index).cloned().ok_or_else(|| {
        AppError::BadRequest("redis hot credit account index out of range".to_string())
    })?;
    let allocation_id = chunk[1].parse::<DbId>().map_err(|err| {
        AppError::BadRequest(format!("invalid redis hot credit allocation id: {err}"))
    })?;
    let amount_micros = chunk[2]
        .parse::<i64>()
        .map_err(|err| AppError::BadRequest(format!("invalid redis hot credit amount: {err}")))?;
    if allocation_id <= 0 || amount_micros <= 0 {
        return Err(AppError::BadRequest(
            "invalid redis hot credit debit part".to_string(),
        ));
    }
    Ok(DebitPart {
        credit_account,
        allocation_id,
        amount_micros,
    })
}

fn decode_hot_segment(credit_account: &CreditAccountId, item: &str) -> Option<DebitPart> {
    // 兼容历史三段格式（id:amount:generation）与当前两段格式：取前两段，忽略多余部分。
    let mut parts = item.splitn(3, ':');
    let allocation_id = parts.next()?.parse::<DbId>().ok()?;
    let amount_micros = parts.next()?.parse::<i64>().ok()?;
    (amount_micros > 0).then(|| DebitPart {
        credit_account: credit_account.clone(),
        allocation_id,
        amount_micros,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hot_credit_store_drains_credit_account() {
        let store = MemoryHotCreditStore::default();
        let credit_account = CreditAccountId::new(7);

        store
            .credit_allocation(credit_account.clone(), 101, 30)
            .await
            .unwrap();
        store
            .credit_allocation(credit_account.clone(), 102, 20)
            .await
            .unwrap();

        let drained = store.drain_credit_account(&credit_account).await.unwrap();
        assert_eq!(drained.len(), 2);
        assert_eq!(
            drained.iter().map(|part| part.amount_micros).sum::<i64>(),
            50
        );
        assert!(store
            .try_debit_ordered(&[credit_account], 1)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn hot_credit_store_removes_allocations() {
        let store = MemoryHotCreditStore::default();
        let credit_account = CreditAccountId::new(7);

        store
            .credit_allocation(credit_account.clone(), 101, 30)
            .await
            .unwrap();
        store
            .credit_allocation(credit_account.clone(), 102, 20)
            .await
            .unwrap();
        store
            .remove_allocations(&[HotAllocation {
                credit_account: credit_account.clone(),
                allocation_id: 101,
            }])
            .await
            .unwrap();

        let drained = store.drain_credit_account(&credit_account).await.unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].allocation_id, 102);
        assert_eq!(drained[0].amount_micros, 20);
    }

    #[tokio::test]
    async fn hot_credit_store_reports_available_credit_across_accounts() {
        let store = MemoryHotCreditStore::default();
        let first = CreditAccountId::new(7);
        let second = CreditAccountId::new(8);

        store
            .credit_allocation(first.clone(), 101, 30)
            .await
            .unwrap();
        store
            .credit_allocation(second.clone(), 102, 20)
            .await
            .unwrap();

        assert_eq!(store.available_micros(&[first, second]).await.unwrap(), 50);
    }
}
