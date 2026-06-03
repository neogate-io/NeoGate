use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use rand::RngExt;
use sha2::{Digest, Sha256};

use crate::id::DbId;

const SECRET_PREFIX: &str = "moli_secret_v1";
const NONCE_LEN: usize = 12;

#[derive(Clone)]
pub struct SecretStore {
    cipher: Aes256Gcm,
    max_cache_entries: usize,
    cache: Arc<RwLock<HashMap<DbId, CachedSecret>>>,
}

#[derive(Clone)]
struct CachedSecret {
    ciphertext: String,
    plaintext: String,
}

impl SecretStore {
    pub fn new(secret_key: &str, max_cache_entries: usize) -> Self {
        let key = Sha256::digest(secret_key.as_bytes());
        let cipher = Aes256Gcm::new_from_slice(&key).expect("sha256 output is a valid AES-256 key");
        Self {
            cipher,
            max_cache_entries,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|_| anyhow!("failed to encrypt upstream secret"))?;
        Ok(format!(
            "{SECRET_PREFIX}:{}:{}",
            STANDARD_NO_PAD.encode(nonce_bytes),
            STANDARD_NO_PAD.encode(ciphertext)
        ))
    }

    pub fn plaintext(&self, key_id: DbId, ciphertext: &str) -> Result<String> {
        if let Some(secret) = self.cached_plaintext(key_id, ciphertext) {
            return Ok(secret);
        }

        let plaintext = self.decrypt(ciphertext)?;
        let mut cache = self.cache.write().expect("secret cache poisoned");
        trim_secret_cache_for_insert(&mut cache, key_id, self.max_cache_entries);
        cache.insert(
            key_id,
            CachedSecret {
                ciphertext: ciphertext.to_string(),
                plaintext: plaintext.clone(),
            },
        );
        Ok(plaintext)
    }

    pub fn forget(&self, key_id: DbId) {
        self.cache
            .write()
            .expect("secret cache poisoned")
            .remove(&key_id);
    }

    fn cached_plaintext(&self, key_id: DbId, ciphertext: &str) -> Option<String> {
        self.cache
            .read()
            .expect("secret cache poisoned")
            .get(&key_id)
            .filter(|cached| cached.ciphertext == ciphertext)
            .map(|cached| cached.plaintext.clone())
    }

    fn decrypt(&self, ciphertext: &str) -> Result<String> {
        let Some(rest) = ciphertext.strip_prefix(SECRET_PREFIX) else {
            return Ok(ciphertext.to_string());
        };
        let mut parts = rest.strip_prefix(':').unwrap_or(rest).splitn(2, ':');
        let nonce = parts.next().context("missing upstream secret nonce")?;
        let encrypted = parts.next().context("missing upstream secret ciphertext")?;
        let nonce = STANDARD_NO_PAD
            .decode(nonce)
            .context("invalid upstream secret nonce")?;
        let encrypted = STANDARD_NO_PAD
            .decode(encrypted)
            .context("invalid upstream secret ciphertext")?;
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(&nonce), encrypted.as_ref())
            .map_err(|_| anyhow!("failed to decrypt upstream secret"))?;
        String::from_utf8(plaintext).context("upstream secret is not valid UTF-8")
    }
}

fn trim_secret_cache_for_insert(
    cache: &mut HashMap<DbId, CachedSecret>,
    keep: DbId,
    max_entries: usize,
) {
    while max_entries > 0 && cache.len() >= max_entries && !cache.contains_key(&keep) {
        let Some(evict) = cache.keys().next().copied() else {
            break;
        };
        cache.remove(&evict);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_roundtrip_uses_ciphertext_prefix() {
        let store = SecretStore::new("test-secret", 4096);
        let encrypted = store.encrypt("sk-test").unwrap();
        assert!(encrypted.starts_with(SECRET_PREFIX));
        assert_ne!(encrypted, "sk-test");
        assert_eq!(store.plaintext(1, &encrypted).unwrap(), "sk-test");
    }

    #[test]
    fn cache_invalidates_when_ciphertext_changes() {
        let store = SecretStore::new("test-secret", 4096);
        let first = store.encrypt("first").unwrap();
        let second = store.encrypt("second").unwrap();
        assert_eq!(store.plaintext(1, &first).unwrap(), "first");
        assert_eq!(store.plaintext(1, &second).unwrap(), "second");
    }
}
