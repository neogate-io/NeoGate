use chrono::{DateTime, Utc};
use sqlx::{postgres::PgRow, Row};

use crate::{
    error::{AppError, AppResult},
    id::DbId,
    input::trimmed_non_empty,
};

pub fn parse_created_id_cursor(
    cursor: Option<&str>,
    invalid_message: &'static str,
) -> AppResult<Option<(DateTime<Utc>, DbId)>> {
    let Some(cursor) = trimmed_non_empty(cursor) else {
        return Ok(None);
    };
    let Some((created_at, id)) = cursor.rsplit_once('|') else {
        return Err(invalid_cursor(invalid_message));
    };
    let created_at = DateTime::parse_from_rfc3339(created_at)
        .map_err(|_| invalid_cursor(invalid_message))?
        .with_timezone(&Utc);
    let id = id
        .parse::<DbId>()
        .map_err(|_| invalid_cursor(invalid_message))?;
    Ok(Some((created_at, id)))
}

pub fn created_id_cursor_from_row(row: &sqlx::postgres::PgRow) -> Result<String, sqlx::Error> {
    let created_at: DateTime<Utc> = row.try_get("created_at")?;
    let id: DbId = row.try_get("id")?;
    Ok(format!("{}|{}", created_at.to_rfc3339(), id))
}

pub fn created_id_cursor_page(
    rows: Vec<PgRow>,
    limit: i64,
) -> Result<(Vec<PgRow>, Option<String>, bool), sqlx::Error> {
    let has_more = rows.len() > limit as usize;
    let rows = rows.into_iter().take(limit as usize).collect::<Vec<_>>();
    let next_cursor = has_more
        .then(|| rows.last())
        .flatten()
        .map(created_id_cursor_from_row)
        .transpose()?;
    Ok((rows, next_cursor, has_more))
}

fn invalid_cursor(message: &'static str) -> AppError {
    AppError::BadRequest(message.to_string())
}
