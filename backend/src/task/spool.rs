use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{fs, io::AsyncWriteExt};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    AppState,
};

use super::REQUEST_SPOOL_TTL;

const SPOOL_DIR: &str = "pending-responses";
const ORPHAN_QUERY_BATCH_SIZE: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Spool {
    pub(crate) path: String,
    bytes: usize,
    sha256: String,
}

pub(crate) async fn save(root: &Path, response_id: &str, body: &[u8]) -> AppResult<Spool> {
    let relative = format!("{SPOOL_DIR}/{response_id}.json");
    let temporary_relative = format!("{SPOOL_DIR}/.{response_id}.{}.tmp", Uuid::new_v4().simple());
    let path = resolve(root, &relative)?;
    let temporary_path = resolve(root, &temporary_relative)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
        #[cfg(unix)]
        fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).await?;
    }

    let result = async {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temporary_path).await?;
        file.write_all(body).await?;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        fs::rename(&temporary_path, &path).await
    }
    .await;
    if let Err(err) = result {
        let _ = fs::remove_file(temporary_path).await;
        return Err(err.into());
    }

    Ok(Spool {
        path: relative,
        bytes: body.len(),
        sha256: digest(body),
    })
}

pub(crate) async fn read(root: &Path, spool: &Spool) -> AppResult<Bytes> {
    let path = resolve(root, &spool.path)?;
    let body = fs::read(&path).await.map_err(|err| {
        AppError::BadRequest(format!("async image request body is missing: {err}"))
    })?;
    if body.len() != spool.bytes || digest(&body) != spool.sha256 {
        return Err(AppError::BadRequest(
            "async image request body failed integrity validation".to_string(),
        ));
    }
    Ok(Bytes::from(body))
}

pub(crate) async fn remove(root: &Path, relative: &str) {
    let Ok(path) = resolve(root, relative) else {
        return;
    };
    if let Err(err) = fs::remove_file(&path).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(path = %path.display(), %err, "failed to remove async image request body");
        }
    }
}

pub(crate) async fn cleanup_orphans(state: &AppState, limit: i64) -> AppResult<u64> {
    let mut entries = match fs::read_dir(state.config.response_assets.dir.join(SPOOL_DIR)).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err.into()),
    };
    let target = u64::try_from(limit.max(1)).unwrap_or(u64::MAX);
    let mut candidates = Vec::with_capacity(ORPHAN_QUERY_BATCH_SIZE);
    let mut deleted = 0;
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await?.is_file() || !older_than_ttl(entry.metadata().await?) {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        candidates.push((
            entry.path(),
            file_name.strip_suffix(".json").map(str::to_string),
        ));
        if candidates.len() == ORPHAN_QUERY_BATCH_SIZE {
            deleted += delete_candidates(&state.db.pool, &mut candidates, target - deleted).await?;
            if deleted >= target {
                return Ok(deleted);
            }
        }
    }
    deleted += delete_candidates(&state.db.pool, &mut candidates, target - deleted).await?;
    Ok(deleted)
}

async fn delete_candidates(
    pool: &sqlx::PgPool,
    candidates: &mut Vec<(PathBuf, Option<String>)>,
    limit: u64,
) -> AppResult<u64> {
    let response_ids = candidates
        .iter()
        .filter_map(|(_, response_id)| response_id.clone())
        .collect::<Vec<_>>();
    let active = active_tasks(pool, &response_ids).await?;

    let mut deleted = 0;
    for (path, response_id) in candidates.drain(..) {
        if deleted >= limit {
            break;
        }
        if response_id.is_some_and(|response_id| active.contains(&response_id)) {
            continue;
        }
        match fs::remove_file(path).await {
            Ok(()) => deleted += 1,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(deleted)
}

async fn active_tasks(pool: &sqlx::PgPool, response_ids: &[String]) -> AppResult<HashSet<String>> {
    if response_ids.is_empty() {
        return Ok(HashSet::new());
    }
    let rows = sqlx::query_scalar::<_, String>(
        r#"
        SELECT upstream_task_id
        FROM task_upstream
        WHERE task_type = 'neogate_response'
          AND upstream_task_id = ANY($1)
          AND terminal = FALSE
        "#,
    )
    .bind(response_ids)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

fn older_than_ttl(metadata: std::fs::Metadata) -> bool {
    metadata
        .modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= REQUEST_SPOOL_TTL)
}

fn digest(body: &[u8]) -> String {
    hex::encode(Sha256::digest(body))
}

fn resolve(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(AppError::BadRequest(
            "invalid async image request spool path".to_string(),
        ));
    }
    Ok(root.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trips_and_removes_file() {
        let root = test_root();
        let body = br#"{"model":"gpt-image-2","prompt":"test"}"#;
        let spool = save(&root, "resp_test", body).await.unwrap();
        let path = resolve(&root, &spool.path).unwrap();

        assert_eq!(spool.bytes, body.len());
        assert_eq!(spool.sha256, digest(body));
        assert!(path.exists());
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        assert_eq!(read(&root, &spool).await.unwrap().as_ref(), body);
        remove(&root, &spool.path).await;
        assert!(!path.exists());
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn rejects_tampered_content() {
        let root = test_root();
        let spool = save(&root, "resp_test", b"original").await.unwrap();
        let path = resolve(&root, &spool.path).unwrap();
        fs::write(&path, b"tampered").await.unwrap();

        let err = read(&root, &spool).await.unwrap_err();
        assert!(err.to_string().contains("integrity validation"));
        assert!(path.exists());
        fs::remove_dir_all(root).await.unwrap();
    }

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "neogate-request-spool-test-{}",
            Uuid::new_v4().simple()
        ))
    }
}
