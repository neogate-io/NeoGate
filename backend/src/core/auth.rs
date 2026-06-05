use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, HeaderMap},
};
use chrono::{DateTime, Utc};
use rand::{
    distr::{Alphanumeric, SampleString},
    RngExt,
};
use sha2::{Digest, Sha256};
use sqlx::Row;

use crate::{
    billing::CreditAccountId,
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

#[derive(Debug, Clone)]
pub struct AdminAuth;

impl FromRequestParts<Arc<AppState>> for AdminAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let has_session_token = bearer(&parts.headers)
            .map(|token| validate_admin_token(token, &state.config.admin_token_secret))
            .unwrap_or(false);

        if !has_session_token {
            return Err(AppError::Unauthorized);
        }
        Ok(Self)
    }
}

pub fn issue_admin_token(ttl: Duration, secret: &str) -> String {
    let expires_at = Utc::now() + chrono_ttl(ttl);
    let expires_at = expires_at.timestamp();
    let nonce = generate_admin_nonce();
    let payload = admin_token_payload(expires_at, &nonce);
    let signature = hmac_sha256_hex(secret.as_bytes(), payload.as_bytes());
    format!("neo_admin_v1_{expires_at}_{nonce}_{signature}")
}

pub fn issue_user_session_token(ttl: Duration, secret: &str, user_id: DbId) -> String {
    let expires_at = Utc::now() + chrono_ttl(ttl);
    let expires_at = expires_at.timestamp();
    let nonce = generate_admin_nonce();
    let payload = user_session_token_payload(expires_at, user_id, &nonce);
    let signature = hmac_sha256_hex(secret.as_bytes(), payload.as_bytes());
    format!("neo_user_v1_{expires_at}_{user_id}_{nonce}_{signature}")
}

pub fn issue_password_reset_token(ttl: Duration, secret: &str, email: &str) -> String {
    let expires_at = Utc::now() + chrono_ttl(ttl);
    let expires_at = expires_at.timestamp();
    let email_hex = hex::encode(email.as_bytes());
    let payload = password_reset_token_payload(expires_at, &email_hex);
    let signature = hmac_sha256_hex(secret.as_bytes(), payload.as_bytes());
    format!("neo_reset_v1_{expires_at}_{email_hex}_{signature}")
}

pub fn password_reset_email_from_token(token: &str, secret: &str) -> Option<String> {
    let rest = token.strip_prefix("neo_reset_v1_")?;
    let mut parts = rest.splitn(3, '_');
    let expires_at = parts.next().and_then(|value| value.parse::<i64>().ok())?;
    let email_hex = parts.next().filter(|value| !value.is_empty())?;
    let signature = parts.next()?;

    if expires_at <= Utc::now().timestamp() {
        return None;
    }

    let payload = password_reset_token_payload(expires_at, email_hex);
    let expected = hmac_sha256_hex(secret.as_bytes(), payload.as_bytes());
    if !constant_time_eq(signature.as_bytes(), expected.as_bytes()) {
        return None;
    }

    let email = hex::decode(email_hex).ok()?;
    String::from_utf8(email).ok()
}

pub fn validate_admin_token(token: &str, secret: &str) -> bool {
    let Some(rest) = token.strip_prefix("neo_admin_v1_") else {
        return false;
    };
    let mut parts = rest.splitn(3, '_');
    let Some(expires_at) = parts.next().and_then(|value| value.parse::<i64>().ok()) else {
        return false;
    };
    let Some(nonce) = parts.next().filter(|value| !value.is_empty()) else {
        return false;
    };
    let Some(signature) = parts.next() else {
        return false;
    };

    if expires_at <= Utc::now().timestamp() {
        return false;
    }

    let payload = admin_token_payload(expires_at, nonce);
    let expected = hmac_sha256_hex(secret.as_bytes(), payload.as_bytes());
    constant_time_eq(signature.as_bytes(), expected.as_bytes())
}

pub fn validate_user_session_token(token: &str, secret: &str) -> Option<DbId> {
    let rest = token.strip_prefix("neo_user_v1_")?;
    let mut parts = rest.splitn(4, '_');
    let expires_at = parts.next().and_then(|value| value.parse::<i64>().ok())?;
    let user_id = parts.next().and_then(|value| value.parse::<DbId>().ok())?;
    let nonce = parts.next().filter(|value| !value.is_empty())?;
    let signature = parts.next()?;

    if expires_at <= Utc::now().timestamp() {
        return None;
    }

    let payload = user_session_token_payload(expires_at, user_id, nonce);
    let expected = hmac_sha256_hex(secret.as_bytes(), payload.as_bytes());
    constant_time_eq(signature.as_bytes(), expected.as_bytes()).then_some(user_id)
}

pub fn hash_user_password(password: &str, secret: &str) -> String {
    let salt = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let digest = user_password_digest(password, secret, &salt);
    format!("neo_pwd_v1${salt}${digest}")
}

pub fn verify_user_password(password: &str, secret: &str, password_hash: &str) -> bool {
    let mut parts = password_hash.split('$');
    let Some("neo_pwd_v1") = parts.next() else {
        return false;
    };
    let Some(salt) = parts.next().filter(|value| !value.is_empty()) else {
        return false;
    };
    let Some(expected) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }

    let digest = user_password_digest(password, secret, salt);
    constant_time_eq(digest.as_bytes(), expected.as_bytes())
}

pub fn hash_email_verification_code(email: &str, code: &str, secret: &str) -> String {
    hmac_sha256_hex(
        secret.as_bytes(),
        format!("login-email-code.v1.{email}.{code}").as_bytes(),
    )
}

pub fn issue_user_key_draft_token(ttl: Duration, secret: &str, head: &str, tail: &str) -> String {
    let expires_at = Utc::now() + chrono_ttl(ttl);
    let expires_at = expires_at.timestamp();
    let payload = user_key_draft_token_payload(expires_at, head, tail);
    let signature = hmac_sha256_hex(secret.as_bytes(), payload.as_bytes());
    format!("neo_draft_v1_{expires_at}_{head}_{tail}_{signature}")
}

pub fn user_key_draft_parts_from_token(
    token: &str,
    secret: &str,
) -> Option<(String, String, String)> {
    let rest = token.strip_prefix("neo_draft_v1_")?;
    let mut parts = rest.splitn(4, '_');
    let expires_at = parts.next()?.parse::<i64>().ok()?;
    let head = parts.next()?;
    let tail = parts.next()?;
    let signature = parts.next()?;

    if expires_at <= Utc::now().timestamp() || !valid_user_key_parts(head, tail) {
        return None;
    }

    let payload = user_key_draft_token_payload(expires_at, head, tail);
    let expected = hmac_sha256_hex(secret.as_bytes(), payload.as_bytes());
    constant_time_eq(signature.as_bytes(), expected.as_bytes())
        .then(|| (head.to_string(), tail.to_string(), signature.to_string()))
}

#[derive(Debug, Clone)]
pub struct UserAuth {
    pub user_id: DbId,
    pub user_key_id: DbId,
    pub user_credit_account: CreditAccountId,
    pub user_key_credit_account: CreditAccountId,
    pub user_group: String,
    pub model_limits: Option<Vec<String>>,
}

#[derive(Clone)]
pub struct UserAuthCache {
    ttl: Duration,
    max_entries_per_shard: usize,
    entries: Arc<Vec<RwLock<HashMap<String, CachedUserAuth>>>>,
}

#[derive(Clone)]
struct CachedUserAuth {
    auth: UserAuth,
    expires_at: Instant,
}

impl UserAuthCache {
    pub fn new(ttl: Duration, max_entries: usize) -> Self {
        let shard_count = max_entries.clamp(1, AUTH_CACHE_SHARDS);
        let max_entries_per_shard = max_entries.div_ceil(shard_count).max(1);
        Self {
            ttl,
            max_entries_per_shard,
            entries: Arc::new(
                (0..shard_count)
                    .map(|_| RwLock::new(HashMap::new()))
                    .collect(),
            ),
        }
    }

    pub fn get(&self, cache_key: &str) -> Option<UserAuth> {
        let shard = self.shard(cache_key);
        {
            let entries = shard.read().expect("user auth cache poisoned");
            let cached = entries.get(cache_key)?;
            if cached.expires_at > Instant::now() {
                return Some(cached.auth.clone());
            }
        }
        let mut entries = shard.write().expect("user auth cache poisoned");
        entries.remove(cache_key);
        None
    }

    pub fn insert(&self, cache_key: String, auth: UserAuth, key_expires_at: Option<DateTime<Utc>>) {
        let Some(ttl) = self.entry_ttl(key_expires_at) else {
            return;
        };
        let now = Instant::now();
        let mut entries = self
            .shard(&cache_key)
            .write()
            .expect("user auth cache poisoned");
        prune_expired_auth_entries(&mut entries, now);
        trim_auth_cache_for_insert(&mut entries, &cache_key, self.max_entries_per_shard);
        entries.insert(
            cache_key,
            CachedUserAuth {
                auth,
                expires_at: now + ttl,
            },
        );
    }

    pub fn remove_user(&self, user_id: DbId) {
        for shard in self.entries.iter() {
            shard
                .write()
                .expect("user auth cache poisoned")
                .retain(|_, cached| cached.auth.user_id != user_id);
        }
    }

    pub fn remove_user_key(&self, user_key_id: DbId) {
        for shard in self.entries.iter() {
            shard
                .write()
                .expect("user auth cache poisoned")
                .retain(|_, cached| cached.auth.user_key_id != user_key_id);
        }
    }

    pub fn clear(&self) {
        for shard in self.entries.iter() {
            shard.write().expect("user auth cache poisoned").clear();
        }
    }

    fn entry_ttl(&self, key_expires_at: Option<DateTime<Utc>>) -> Option<Duration> {
        let mut ttl = self.ttl;
        if let Some(expires_at) = key_expires_at {
            let remaining = expires_at - Utc::now();
            if remaining <= chrono::Duration::zero() {
                return None;
            }
            let remaining = remaining.to_std().ok()?;
            ttl = ttl.min(remaining);
        }
        (!ttl.is_zero()).then_some(ttl)
    }

    fn shard(&self, cache_key: &str) -> &RwLock<HashMap<String, CachedUserAuth>> {
        let mut hasher = DefaultHasher::new();
        cache_key.hash(&mut hasher);
        &self.entries[hasher.finish() as usize % self.entries.len()]
    }
}

fn prune_expired_auth_entries(entries: &mut HashMap<String, CachedUserAuth>, now: Instant) {
    entries.retain(|_, cached| cached.expires_at > now);
}

fn trim_auth_cache_for_insert(
    entries: &mut HashMap<String, CachedUserAuth>,
    keep: &str,
    max_entries: usize,
) {
    while max_entries > 0 && entries.len() >= max_entries && !entries.contains_key(keep) {
        let Some(evict) = entries.keys().next().cloned() else {
            break;
        };
        entries.remove(&evict);
    }
}

impl FromRequestParts<Arc<AppState>> for UserAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let raw_key = bearer(&parts.headers)
            .or_else(|| parts.headers.get("x-api-key").and_then(|v| v.to_str().ok()))
            .ok_or(AppError::Unauthorized)?;
        let cache_key = hash_key(raw_key);
        if let Some(auth) = state.user_auth_cache.get(&cache_key) {
            return Ok(auth);
        }

        let rows = sqlx::query(
            r#"
            SELECT uk.id AS user_key_id, uk.user_id, uk.status AS key_status,
                   uk.secret_ciphertext, uk.expires_at, uk.model_limits, u.status AS user_status,
                   uw.id AS user_credit_account_id, ukw.id AS user_key_credit_account_id,
                   ug.code AS user_group
            FROM user_key uk
            JOIN "user" u ON u.id = uk.user_id
            JOIN user_group ug ON ug.id = u.user_group_id
            JOIN credit_account uw ON uw.owner_type = 'user' AND uw.owner_id = u.id
            JOIN credit_account ukw ON ukw.owner_type = 'user_key' AND ukw.owner_id = uk.id
            WHERE uk.key_prefix = $1
            "#,
        )
        .bind(key_prefix(raw_key))
        .fetch_all(&state.db.pool)
        .await?;

        let mut matched = None;
        for row in rows {
            let user_key_id: DbId = row.try_get("user_key_id")?;
            let secret_ciphertext: String = row.try_get("secret_ciphertext")?;
            let stored_key = state.secrets.plaintext(user_key_id, &secret_ciphertext)?;
            if constant_time_eq(stored_key.as_bytes(), raw_key.as_bytes()) {
                matched = Some(row);
                break;
            }
        }

        let row = matched.ok_or(AppError::Unauthorized)?;
        let user_status: String = row.try_get("user_status")?;
        let key_status: String = row.try_get("key_status")?;
        if user_status != "enabled" || key_status != "enabled" {
            return Err(AppError::Forbidden);
        }
        let expires_at: Option<DateTime<Utc>> = row.try_get("expires_at")?;
        if expires_at.map(|value| value <= Utc::now()).unwrap_or(false) {
            return Err(AppError::Forbidden);
        }

        let auth = Self {
            user_id: row.try_get("user_id")?,
            user_key_id: row.try_get("user_key_id")?,
            user_credit_account: CreditAccountId::new(row.try_get("user_credit_account_id")?),
            user_key_credit_account: CreditAccountId::new(
                row.try_get("user_key_credit_account_id")?,
            ),
            user_group: row.try_get("user_group")?,
            model_limits: row.try_get("model_limits")?,
        };
        state
            .user_auth_cache
            .insert(cache_key, auth.clone(), expires_at);
        Ok(auth)
    }
}

impl UserAuth {
    pub fn ensure_model_allowed(&self, model: &str) -> AppResult<()> {
        if let Some(limits) = &self.model_limits {
            if !limits.iter().any(|item| item == model) {
                return Err(AppError::Forbidden);
            }
        }
        Ok(())
    }
}

pub fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(http::header::AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ")
}

pub fn generate_user_key() -> String {
    format!("sk-{}", generate_user_key_chars(USER_KEY_SUFFIX_LEN))
}

#[cfg(test)]
pub fn generate_user_key_from_parts(head: &str, tail: &str) -> Option<String> {
    if !valid_user_key_parts(head, tail) {
        return None;
    }

    let middle_len = USER_KEY_TOTAL_LEN - head.len() - tail.len();
    Some(format!(
        "{head}{}{tail}",
        generate_user_key_chars(middle_len)
    ))
}

pub fn generate_user_key_from_parts_and_seed(head: &str, tail: &str, seed: &str) -> Option<String> {
    if !valid_user_key_parts(head, tail) {
        return None;
    }

    let middle_len = USER_KEY_TOTAL_LEN - head.len() - tail.len();
    Some(format!(
        "{head}{}{tail}",
        generate_seeded_user_key_chars(middle_len, seed)
    ))
}

fn generate_user_key_chars(len: usize) -> String {
    let mut rng = rand::rng();

    (0..len)
        .map(|_| KEY_CHARS[rng.random_range(0..KEY_CHARS.len())] as char)
        .collect()
}

fn generate_seeded_user_key_chars(len: usize, seed: &str) -> String {
    let mut output = String::with_capacity(len);
    let mut counter = 0u64;
    while output.len() < len {
        let mut hasher = Sha256::new();
        hasher.update(seed.as_bytes());
        hasher.update(counter.to_be_bytes());
        for byte in hasher.finalize() {
            if output.len() == len {
                break;
            }
            output.push(KEY_CHARS[byte as usize % KEY_CHARS.len()] as char);
        }
        counter += 1;
    }
    output
}

pub fn is_generated_user_key(secret: &str) -> bool {
    let Some(suffix) = secret.strip_prefix(USER_KEY_PREFIX) else {
        return false;
    };
    suffix.len() == USER_KEY_SUFFIX_LEN && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn generate_admin_nonce() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 48)
}

fn admin_token_payload(expires_at: i64, nonce: &str) -> String {
    format!("v1.{expires_at}.{nonce}")
}

fn user_key_draft_token_payload(expires_at: i64, head: &str, tail: &str) -> String {
    format!("draft.v1.{expires_at}.{head}.{tail}")
}

fn user_session_token_payload(expires_at: i64, user_id: DbId, nonce: &str) -> String {
    format!("user.v1.{expires_at}.{user_id}.{nonce}")
}

fn password_reset_token_payload(expires_at: i64, email_hex: &str) -> String {
    format!("reset.v1.{expires_at}.{email_hex}")
}

fn user_password_digest(password: &str, secret: &str, salt: &str) -> String {
    let mut digest = hmac_sha256_hex(secret.as_bytes(), format!("{salt}:{password}").as_bytes());
    for _ in 0..10_000 {
        digest = hmac_sha256_hex(secret.as_bytes(), format!("{salt}:{digest}").as_bytes());
    }
    digest
}

fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    const BLOCK_SIZE: usize = 64;
    let mut key_block = if key.len() > BLOCK_SIZE {
        Sha256::digest(key).to_vec()
    } else {
        key.to_vec()
    };
    key_block.resize(BLOCK_SIZE, 0);

    let mut outer = [0x5c; BLOCK_SIZE];
    let mut inner = [0x36; BLOCK_SIZE];
    for index in 0..BLOCK_SIZE {
        outer[index] ^= key_block[index];
        inner[index] ^= key_block[index];
    }

    let mut inner_hasher = Sha256::new();
    inner_hasher.update(inner);
    inner_hasher.update(message);
    let inner_hash = inner_hasher.finalize();

    let mut outer_hasher = Sha256::new();
    outer_hasher.update(outer);
    outer_hasher.update(inner_hash);
    hex_lower(outer_hasher.finalize())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0;
    for (left, right) in left.iter().zip(right) {
        diff |= *left ^ *right;
    }
    diff == 0
}

fn valid_user_key_parts(head: &str, tail: &str) -> bool {
    head.starts_with(USER_KEY_PREFIX)
        && head.len() + tail.len() <= USER_KEY_TOTAL_LEN
        && head[USER_KEY_PREFIX.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric())
        && tail.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn chrono_ttl(ttl: Duration) -> chrono::Duration {
    chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::hours(24))
}

pub fn key_prefix(secret: &str) -> String {
    secret.chars().take(12).collect()
}

pub fn hash_key(secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hex_lower(hasher.finalize())
}

fn hex_lower(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

const USER_KEY_PREFIX: &str = "sk-";
const USER_KEY_SUFFIX_LEN: usize = 48;
const USER_KEY_TOTAL_LEN: usize = USER_KEY_PREFIX.len() + USER_KEY_SUFFIX_LEN;
const AUTH_CACHE_SHARDS: usize = 64;
const KEY_CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_prefix_is_short_and_stable() {
        assert_eq!(key_prefix("neo_abcdefghijklmnopqrstuvwxyz"), "neo_abcdefgh");
    }

    #[test]
    fn hash_key_is_sha256_hex() {
        assert_eq!(
            hash_key("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn user_keys_match_new_api_shape() {
        let key = generate_user_key();

        assert_eq!(key.len(), 51);
        assert!(key.starts_with("sk-"));
        assert!(is_generated_user_key(&key));
        assert!(!is_generated_user_key(&key[3..]));
        assert!(!is_generated_user_key("neo_abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn user_key_can_be_recreated_from_preview_parts() {
        let original = generate_user_key();
        let recreated =
            generate_user_key_from_parts(&original[..18], &original[original.len() - 10..])
                .unwrap();

        assert!(is_generated_user_key(&recreated));
        assert_eq!(&recreated[..18], &original[..18]);
        assert_eq!(
            &recreated[recreated.len() - 10..],
            &original[original.len() - 10..]
        );
    }

    #[test]
    fn user_key_draft_tokens_are_signed_and_expire() {
        let secret = "test-draft-token-secret";
        let original = generate_user_key();
        let head = &original[..18];
        let tail = &original[original.len() - 10..];
        let token = issue_user_key_draft_token(Duration::from_secs(60), secret, head, tail);

        let (parsed_head, parsed_tail, signature) =
            user_key_draft_parts_from_token(&token, secret).unwrap();
        assert_eq!(parsed_head, head);
        assert_eq!(parsed_tail, tail);
        assert!(
            generate_user_key_from_parts_and_seed(&parsed_head, &parsed_tail, &signature).is_some()
        );
        assert!(user_key_draft_parts_from_token(&token, "different-secret").is_none());

        let expired = issue_user_key_draft_token(Duration::from_secs(0), secret, head, tail);
        assert!(user_key_draft_parts_from_token(&expired, secret).is_none());
    }

    #[test]
    fn model_limits_reject_unlisted_models() {
        let auth = UserAuth {
            user_id: 1,
            user_key_id: 1,
            user_credit_account: CreditAccountId::new(100),
            user_key_credit_account: CreditAccountId::new(101),
            user_group: "default".to_string(),
            model_limits: Some(vec!["gpt-4.1".to_string()]),
        };
        assert!(auth.ensure_model_allowed("gpt-4.1").is_ok());
        assert!(auth.ensure_model_allowed("claude-3-5-sonnet").is_err());
    }

    #[test]
    fn user_auth_cache_removes_targeted_entries() {
        let cache = UserAuthCache::new(Duration::from_secs(60), 1024);
        cache.insert(
            "key-a".to_string(),
            UserAuth {
                user_id: 1,
                user_key_id: 10,
                user_credit_account: CreditAccountId::new(100),
                user_key_credit_account: CreditAccountId::new(110),
                user_group: "default".to_string(),
                model_limits: None,
            },
            None,
        );
        cache.insert(
            "key-b".to_string(),
            UserAuth {
                user_id: 1,
                user_key_id: 11,
                user_credit_account: CreditAccountId::new(100),
                user_key_credit_account: CreditAccountId::new(111),
                user_group: "default".to_string(),
                model_limits: None,
            },
            None,
        );
        cache.insert(
            "key-c".to_string(),
            UserAuth {
                user_id: 2,
                user_key_id: 20,
                user_credit_account: CreditAccountId::new(200),
                user_key_credit_account: CreditAccountId::new(120),
                user_group: "default".to_string(),
                model_limits: None,
            },
            None,
        );

        cache.remove_user_key(10);
        assert!(cache.get("key-a").is_none());
        assert!(cache.get("key-b").is_some());
        assert!(cache.get("key-c").is_some());

        cache.remove_user(1);
        assert!(cache.get("key-b").is_none());
        assert!(cache.get("key-c").is_some());
    }

    #[test]
    fn admin_tokens_are_signed_and_expire() {
        let secret = "test-admin-token-secret";
        let token = issue_admin_token(Duration::from_secs(60), secret);

        assert!(token.starts_with("neo_admin_"));
        assert!(validate_admin_token(&token, secret));
        assert!(!validate_admin_token(&token, "different-secret"));
        assert!(!validate_admin_token("change-me-in-production", secret));

        let expired = issue_admin_token(Duration::from_secs(0), secret);
        assert!(!validate_admin_token(&expired, secret));
    }

    #[test]
    fn user_session_tokens_are_signed_and_include_user_id() {
        let secret = "test-user-token-secret";
        let token = issue_user_session_token(Duration::from_secs(60), secret, 42);

        assert!(token.starts_with("neo_user_"));
        assert_eq!(validate_user_session_token(&token, secret), Some(42));
        assert_eq!(
            validate_user_session_token(&token, "different-secret"),
            None
        );

        let expired = issue_user_session_token(Duration::from_secs(0), secret, 42);
        assert_eq!(validate_user_session_token(&expired, secret), None);
    }

    #[test]
    fn password_reset_tokens_are_signed_and_include_email() {
        let secret = "test-password-reset-secret";
        let token =
            issue_password_reset_token(Duration::from_secs(60), secret, "user_name@example.com");

        assert!(token.starts_with("neo_reset_"));
        assert_eq!(
            password_reset_email_from_token(&token, secret).as_deref(),
            Some("user_name@example.com")
        );
        assert!(password_reset_email_from_token(&token, "different-secret").is_none());

        let expired =
            issue_password_reset_token(Duration::from_secs(0), secret, "user@example.com");
        assert!(password_reset_email_from_token(&expired, secret).is_none());
    }
}
