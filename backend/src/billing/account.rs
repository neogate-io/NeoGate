use sqlx::{Postgres, Row, Transaction};

use crate::{
    error::{AppError, AppResult},
    id::DbId,
};

use super::{CreditAccountId, CreditAccountType};

pub(crate) async fn create_credit_account(
    tx: &mut Transaction<'_, Postgres>,
    credit_account_type: CreditAccountType,
    owner_id: DbId,
) -> AppResult<CreditAccountId> {
    let row = sqlx::query(
        "INSERT INTO credit_account (owner_type, owner_id)
         VALUES ($1, $2)
         RETURNING id",
    )
    .bind(credit_account_type.as_str())
    .bind(owner_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(CreditAccountId::new(row.try_get("id")?))
}

pub(crate) async fn get_or_create_credit_account_for_update(
    tx: &mut Transaction<'_, Postgres>,
    credit_account_type: CreditAccountType,
    owner_id: DbId,
) -> AppResult<CreditAccountId> {
    let row = sqlx::query(
        "INSERT INTO credit_account (owner_type, owner_id)
         VALUES ($1, $2)
         ON CONFLICT (owner_type, owner_id)
         DO UPDATE SET owner_type = EXCLUDED.owner_type
         RETURNING id",
    )
    .bind(credit_account_type.as_str())
    .bind(owner_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(CreditAccountId::new(row.try_get("id")?))
}

pub(crate) async fn owner_credit_account_for_update(
    tx: &mut Transaction<'_, Postgres>,
    credit_account_type: CreditAccountType,
    owner_id: DbId,
) -> AppResult<CreditAccountId> {
    let row = sqlx::query(
        "SELECT id
         FROM credit_account
         WHERE owner_type = $1 AND owner_id = $2
         FOR UPDATE",
    )
    .bind(credit_account_type.as_str())
    .bind(owner_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(CreditAccountId::new(row.try_get("id")?))
}

pub(crate) async fn lock_for_update(
    tx: &mut Transaction<'_, Postgres>,
    credit_account: &CreditAccountId,
) -> AppResult<()> {
    sqlx::query("SELECT id FROM credit_account WHERE id = $1 FOR UPDATE")
        .bind(credit_account.id)
        .fetch_optional(&mut **tx)
        .await?
        .map(|_| ())
        .ok_or(AppError::NotFound)
}

pub(crate) async fn adjust_balance(
    tx: &mut Transaction<'_, Postgres>,
    credit_account: &CreditAccountId,
    amount_micros: i64,
) -> AppResult<i64> {
    let row = sqlx::query(
        "UPDATE credit_account
         SET balance_micros = balance_micros + $2,
             updated_at = now()
         WHERE id = $1 AND balance_micros + $2 >= reserved_micros
         RETURNING balance_micros",
    )
    .bind(credit_account.id)
    .bind(amount_micros)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AppError::PaymentRequired)?;

    Ok(row.try_get("balance_micros")?)
}

pub(crate) async fn decrement_reserved(
    tx: &mut Transaction<'_, Postgres>,
    credit_account: &CreditAccountId,
    amount_micros: i64,
) -> AppResult<()> {
    decrement_reserved_returning_balance(tx, credit_account, amount_micros)
        .await
        .map(|_| ())
}

pub(crate) async fn decrement_reserved_returning_balance(
    tx: &mut Transaction<'_, Postgres>,
    credit_account: &CreditAccountId,
    amount_micros: i64,
) -> AppResult<i64> {
    let row = sqlx::query(
        "UPDATE credit_account
         SET reserved_micros = reserved_micros - $2,
             updated_at = now()
         WHERE id = $1
           AND reserved_micros >= $2
         RETURNING balance_micros",
    )
    .bind(credit_account.id)
    .bind(amount_micros)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AppError::Conflict(
        "reserved credit is insufficient".to_string(),
    ))?;
    Ok(row.try_get("balance_micros")?)
}

pub(crate) async fn debit_reserved_balance(
    tx: &mut Transaction<'_, Postgres>,
    credit_account: &CreditAccountId,
    amount_micros: i64,
) -> AppResult<i64> {
    let row = sqlx::query(
        "UPDATE credit_account
         SET balance_micros = balance_micros - $2,
             reserved_micros = reserved_micros - $2,
             updated_at = now()
         WHERE id = $1
           AND balance_micros >= $2
           AND reserved_micros >= $2
         RETURNING balance_micros",
    )
    .bind(credit_account.id)
    .bind(amount_micros)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AppError::Conflict(
        "reserved credit is insufficient".to_string(),
    ))?;
    Ok(row.try_get("balance_micros")?)
}

pub(crate) async fn mark_allocation_returned(
    tx: &mut Transaction<'_, Postgres>,
    allocation_id: DbId,
    amount_micros: i64,
) -> AppResult<()> {
    let result = sqlx::query(
        "UPDATE credit_allocation
         SET returned_micros = returned_micros + $2,
             status = CASE
                 WHEN consumed_micros + returned_micros + $2 >= amount_micros
                 THEN 'settled'
                 ELSE status
             END,
             updated_at = now()
         WHERE id = $1
           AND consumed_micros + returned_micros + $2 <= amount_micros",
    )
    .bind(allocation_id)
    .bind(amount_micros)
    .execute(&mut **tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "credit allocation return exceeds available amount".to_string(),
        ));
    }
    Ok(())
}

pub(crate) async fn mark_allocation_consumed(
    tx: &mut Transaction<'_, Postgres>,
    allocation_id: DbId,
    amount_micros: i64,
) -> AppResult<()> {
    let result = sqlx::query(
        "UPDATE credit_allocation
         SET consumed_micros = consumed_micros + $2,
             status = CASE
                 WHEN consumed_micros + returned_micros + $2 >= amount_micros
                 THEN 'settled'
                 ELSE status
             END,
             updated_at = now()
         WHERE id = $1
           AND consumed_micros + returned_micros + $2 <= amount_micros",
    )
    .bind(allocation_id)
    .bind(amount_micros)
    .execute(&mut **tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "credit allocation consume exceeds available amount".to_string(),
        ));
    }
    Ok(())
}

/// Whether the allocation was already recovered by the stale-allocation job.
///
/// The recovery job returns the held credit to the account when a hold outlives
/// the configured credit allocation recovery window (which can happen for
/// long-running requests,
/// since the billing row is only written at settle time, after the upstream
/// call completes). When the outbox later tries to consume such an allocation,
/// the credit has already been refunded, so consuming would either double-charge
/// or hit the capacity constraint and fail permanently. Callers should skip the
/// charge instead — see `flush_billing_part`.
///
/// Uses `FOR UPDATE` so the recovery job's competing `UPDATE ... WHERE status =
/// 'active'` is serialized against this decision, eliminating the check-then-act
/// race.
pub(crate) async fn allocation_is_recovered(
    tx: &mut Transaction<'_, Postgres>,
    allocation_id: DbId,
) -> AppResult<bool> {
    let row = sqlx::query(
        "SELECT status = 'recovered' AS recovered
         FROM credit_allocation
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(allocation_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.is_some_and(|row| row.try_get("recovered").unwrap_or(false)))
}

pub(crate) async fn mark_allocation_recovered(
    tx: &mut Transaction<'_, Postgres>,
    allocation_id: DbId,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE credit_allocation
         SET returned_micros = amount_micros - consumed_micros,
             status = 'recovered',
             updated_at = now()
         WHERE id = $1 AND status = 'active'",
    )
    .bind(allocation_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
