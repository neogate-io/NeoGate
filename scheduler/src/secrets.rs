use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use sha2::{Digest, Sha256};

const SECRET_PREFIX: &str = "neo_secret_v1";

#[derive(Clone)]
pub(crate) struct SecretStore {
    cipher: Aes256Gcm,
}

impl SecretStore {
    pub fn new(secret_key: &str) -> Self {
        let key = Sha256::digest(secret_key.as_bytes());
        let cipher = Aes256Gcm::new_from_slice(&key).expect("sha256 output is a valid AES-256 key");
        Self { cipher }
    }

    pub fn plaintext(&self, ciphertext: &str) -> Result<String> {
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
