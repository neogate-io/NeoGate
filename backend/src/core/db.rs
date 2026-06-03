use anyhow::Result;
use sqlx::{postgres::PgPoolOptions, PgPool};

use crate::config::Config;

#[derive(Clone)]
pub struct Db {
    pub pool: PgPool,
}

impl Db {
    pub async fn connect(config: &Config) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .min_connections(config.db_pool.min_connections)
            .max_connections(config.db_pool.max_connections)
            .acquire_timeout(config.db_pool.acquire_timeout)
            .connect(&config.database_url)
            .await?;
        Ok(Self { pool })
    }
}
