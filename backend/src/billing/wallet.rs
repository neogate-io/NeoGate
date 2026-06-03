use sqlx::{PgPool, Postgres, Row, Transaction};

use crate::{
    error::{AppError, AppResult},
    id::DbId,
};

use super::{WalletId, WalletType};

pub(crate) async fn create_owner_wallet(
    tx: &mut Transaction<'_, Postgres>,
    wallet_type: WalletType,
    owner_id: DbId,
) -> AppResult<WalletId> {
    let row = sqlx::query(
        "INSERT INTO wallet (owner_type, owner_id)
         VALUES ($1, $2)
         RETURNING id",
    )
    .bind(wallet_type.as_str())
    .bind(owner_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(WalletId::new(row.try_get("id")?))
}

pub(crate) async fn owner_wallet(
    pool: &PgPool,
    wallet_type: WalletType,
    owner_id: DbId,
) -> AppResult<WalletId> {
    let row = sqlx::query(
        "SELECT id
         FROM wallet
         WHERE owner_type = $1 AND owner_id = $2",
    )
    .bind(wallet_type.as_str())
    .bind(owner_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(WalletId::new(row.try_get("id")?))
}

pub(crate) async fn owner_wallet_for_update(
    tx: &mut Transaction<'_, Postgres>,
    wallet_type: WalletType,
    owner_id: DbId,
) -> AppResult<WalletId> {
    let row = sqlx::query(
        "SELECT id
         FROM wallet
         WHERE owner_type = $1 AND owner_id = $2
         FOR UPDATE",
    )
    .bind(wallet_type.as_str())
    .bind(owner_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(WalletId::new(row.try_get("id")?))
}

pub(crate) async fn lock_for_update(
    tx: &mut Transaction<'_, Postgres>,
    wallet: &WalletId,
) -> AppResult<()> {
    sqlx::query("SELECT id FROM wallet WHERE id = $1 FOR UPDATE")
        .bind(wallet.id)
        .fetch_optional(&mut **tx)
        .await?
        .map(|_| ())
        .ok_or(AppError::NotFound)
}

pub(crate) async fn adjust_balance(
    tx: &mut Transaction<'_, Postgres>,
    wallet: &WalletId,
    amount_micro_usd: i64,
) -> AppResult<i64> {
    let row = sqlx::query(
        "UPDATE wallet
         SET balance_micro_usd = balance_micro_usd + $2,
             updated_at = now()
         WHERE id = $1 AND balance_micro_usd + $2 >= reserved_micro_usd
         RETURNING balance_micro_usd",
    )
    .bind(wallet.id)
    .bind(amount_micro_usd)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AppError::PaymentRequired)?;

    Ok(row.try_get("balance_micro_usd")?)
}

pub(crate) async fn decrement_reserved(
    tx: &mut Transaction<'_, Postgres>,
    wallet: &WalletId,
    amount_micro_usd: i64,
) -> AppResult<()> {
    decrement_reserved_returning_balance(tx, wallet, amount_micro_usd)
        .await
        .map(|_| ())
}

pub(crate) async fn decrement_reserved_returning_balance(
    tx: &mut Transaction<'_, Postgres>,
    wallet: &WalletId,
    amount_micro_usd: i64,
) -> AppResult<i64> {
    let row = sqlx::query(
        "UPDATE wallet
         SET reserved_micro_usd = reserved_micro_usd - $2,
             updated_at = now()
         WHERE id = $1
         RETURNING balance_micro_usd",
    )
    .bind(wallet.id)
    .bind(amount_micro_usd)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row.try_get("balance_micro_usd")?)
}

pub(crate) async fn debit_reserved_balance(
    tx: &mut Transaction<'_, Postgres>,
    wallet: &WalletId,
    amount_micro_usd: i64,
) -> AppResult<i64> {
    let row = sqlx::query(
        "UPDATE wallet
         SET balance_micro_usd = balance_micro_usd - $2,
             reserved_micro_usd = reserved_micro_usd - $2,
             updated_at = now()
         WHERE id = $1
         RETURNING balance_micro_usd",
    )
    .bind(wallet.id)
    .bind(amount_micro_usd)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row.try_get("balance_micro_usd")?)
}

pub(crate) async fn mark_allocation_returned(
    tx: &mut Transaction<'_, Postgres>,
    allocation_id: DbId,
    amount_micro_usd: i64,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE credit_allocation
         SET returned_micro_usd = returned_micro_usd + $2,
             status = CASE
                 WHEN consumed_micro_usd + returned_micro_usd + $2 >= amount_micro_usd
                 THEN 'settled'
                 ELSE status
             END,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(allocation_id)
    .bind(amount_micro_usd)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) async fn mark_allocation_consumed(
    tx: &mut Transaction<'_, Postgres>,
    allocation_id: DbId,
    amount_micro_usd: i64,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE credit_allocation
         SET consumed_micro_usd = consumed_micro_usd + $2,
             status = CASE
                 WHEN consumed_micro_usd + returned_micro_usd + $2 >= amount_micro_usd
                 THEN 'settled'
                 ELSE status
             END,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(allocation_id)
    .bind(amount_micro_usd)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) async fn mark_allocation_recovered(
    tx: &mut Transaction<'_, Postgres>,
    allocation_id: DbId,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE credit_allocation
         SET returned_micro_usd = amount_micro_usd - consumed_micro_usd,
             status = 'recovered',
             updated_at = now()
         WHERE id = $1 AND status = 'active'",
    )
    .bind(allocation_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
