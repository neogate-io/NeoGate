use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use uuid::Uuid;

async fn test_pool() -> Option<PgPool> {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("skipping database integration test; TEST_DATABASE_URL is not set");
        return None;
    };

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("connect to TEST_DATABASE_URL");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("run migrations against TEST_DATABASE_URL");
    Some(pool)
}

#[tokio::test]
async fn migrations_seed_required_provider_rows() {
    let Some(pool) = test_pool().await else {
        return;
    };

    let row = sqlx::query("SELECT COUNT(*)::BIGINT AS count FROM provider WHERE code = 'openai'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let count: i64 = row.try_get("count").unwrap();

    assert_eq!(count, 1);
}

#[tokio::test]
async fn credit_account_balance_constraints_are_enforced() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let owner_id = random_owner_id();

    let result = sqlx::query(
        "INSERT INTO credit_account
         (owner_type, owner_id, balance_micro_usd, reserved_micro_usd)
         VALUES ('user', $1, 100, 101)",
    )
    .bind(owner_id)
    .execute(&pool)
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn billing_transaction_id_is_idempotent() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let transaction_id = Uuid::new_v4();
    let payload = serde_json::json!({ "test": true, "transaction_id": transaction_id });

    for _ in 0..2 {
        sqlx::query(
            "INSERT INTO billing (transaction_id, payload)
             VALUES ($1, $2)
             ON CONFLICT (transaction_id) DO NOTHING",
        )
        .bind(transaction_id)
        .bind(&payload)
        .execute(&pool)
        .await
        .unwrap();
    }

    let row =
        sqlx::query("SELECT COUNT(*)::BIGINT AS count FROM billing WHERE transaction_id = $1")
            .bind(transaction_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let count: i64 = row.try_get("count").unwrap();

    assert_eq!(count, 1);
}

fn random_owner_id() -> i64 {
    (Uuid::new_v4().as_u128() % 9_000_000_000_000_000_000u128) as i64
}
