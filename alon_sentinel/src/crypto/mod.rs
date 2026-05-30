use aes_gcm_siv::{
    Aes256GcmSiv,
    aead::{Aead, KeyInit},
};
use anyhow::{Context, Result, anyhow, bail};
use hmac::{Hmac, Mac};
use sha2::Sha256;

const WEBHOOK_SECRET_NONCE_LEN: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebhookSecretEncryptionKey([u8; 32]);

impl WebhookSecretEncryptionKey {
    pub fn from_hex(hex_value: &str) -> Result<Self> {
        let key_bytes = hex::decode(hex_value.trim())
            .map_err(|error| anyhow!("WEBHOOK_SECRET_ENCRYPTION_KEY must be valid hex: {error}"))?;
        let key_bytes: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| anyhow!("WEBHOOK_SECRET_ENCRYPTION_KEY must be exactly 32 bytes"))?;

        Ok(Self(key_bytes))
    }

    pub fn encrypt_webhook_secret(&self, plaintext: &str) -> Result<String> {
        if plaintext.is_empty() {
            bail!("webhook secret can not be empty");
        }

        let cipher = Aes256GcmSiv::new_from_slice(&self.0)
            .map_err(|_| anyhow!("invalid webhook secret encryption key length"))?;
        let nonce_bytes = rand::random::<[u8; WEBHOOK_SECRET_NONCE_LEN]>();
        let ciphertext = cipher
            .encrypt(
                aes_gcm_siv::Nonce::from_slice(&nonce_bytes),
                plaintext.as_bytes(),
            )
            .map_err(|_| anyhow!("failed to encrypt webhook secret"))?;

        let mut encoded = Vec::with_capacity(WEBHOOK_SECRET_NONCE_LEN + ciphertext.len());
        encoded.extend_from_slice(&nonce_bytes);
        encoded.extend_from_slice(&ciphertext);

        Ok(hex::encode(encoded))
    }

    pub fn decrypt_webhook_secret(&self, ciphertext_hex: &str) -> Result<String> {
        let encoded = hex::decode(ciphertext_hex.trim()).map_err(|error| {
            anyhow!("stored webhook secret ciphertext is not valid hex: {error}")
        })?;
        if encoded.len() <= WEBHOOK_SECRET_NONCE_LEN {
            bail!("stored webhook secret ciphertext is truncated");
        }

        let (nonce_bytes, ciphertext) = encoded.split_at(WEBHOOK_SECRET_NONCE_LEN);
        let cipher = Aes256GcmSiv::new_from_slice(&self.0)
            .map_err(|_| anyhow!("invalid webhook secret encryption key length"))?;
        let plaintext = cipher
            .decrypt(aes_gcm_siv::Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| anyhow!("failed to decrypt webhook secret"))?;

        String::from_utf8(plaintext).context("decrypted webhook secret is not valid UTF-8")
    }
}

pub fn build_webhook_signature(secret: &str, timestamp: &str, body: &[u8]) -> String {
    let mut message = Vec::with_capacity(timestamp.len() + 1 + body.len());
    message.extend_from_slice(timestamp.as_bytes());
    message.push(b'.');
    message.extend_from_slice(body);

    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts arbitrary key sizes");
    mac.update(&message);

    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{WebhookSecretEncryptionKey, build_webhook_signature};

    const TEST_KEY_HEX: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn test_key() -> WebhookSecretEncryptionKey {
        WebhookSecretEncryptionKey::from_hex(TEST_KEY_HEX).expect("test key should parse")
    }

    #[test]
    fn from_hex_fails_for_invalid_hex_characters() {
        let result = WebhookSecretEncryptionKey::from_hex("not-valid-hex!!");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("valid hex"), "unexpected message: {msg}");
    }

    #[test]
    fn from_hex_fails_when_key_is_too_short() {
        // 16 bytes = 32 hex chars — valid hex but wrong length
        let result = WebhookSecretEncryptionKey::from_hex("00112233445566778899aabbccddeeff");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("32 bytes"), "unexpected message: {msg}");
    }

    #[test]
    fn encrypt_webhook_secret_rejects_empty_secret() {
        let result = test_key().encrypt_webhook_secret("");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("empty"), "unexpected message: {msg}");
    }

    #[test]
    fn decrypt_webhook_secret_fails_for_invalid_hex() {
        let result = test_key().decrypt_webhook_secret("not-valid-hex!!");
        assert!(result.is_err());
    }

    #[test]
    fn decrypt_webhook_secret_fails_for_truncated_ciphertext() {
        // Exactly 12 bytes (24 hex chars) = only the nonce, no payload
        let result = test_key().decrypt_webhook_secret("000000000000000000000000");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("truncated"), "unexpected message: {msg}");
    }

    #[test]
    fn decrypt_webhook_secret_fails_with_wrong_key() {
        let other_key = WebhookSecretEncryptionKey::from_hex(
            "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100",
        )
        .expect("key should parse");
        let ciphertext = test_key()
            .encrypt_webhook_secret("my-secret")
            .expect("should encrypt");
        assert!(other_key.decrypt_webhook_secret(&ciphertext).is_err());
    }

    #[test]
    fn webhook_secret_round_trips_through_encryption() {
        let key = WebhookSecretEncryptionKey::from_hex(
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
        )
        .expect("key should parse");

        let ciphertext = key
            .encrypt_webhook_secret("super-secret-value")
            .expect("secret should encrypt");
        assert_ne!(ciphertext, "super-secret-value");

        let plaintext = key
            .decrypt_webhook_secret(&ciphertext)
            .expect("secret should decrypt");
        assert_eq!(plaintext, "super-secret-value");
    }

    #[test]
    fn webhook_signature_is_stable() {
        let signature =
            build_webhook_signature("hook-secret", "1714300000", br#"{"hello":"world"}"#);

        assert_eq!(
            signature,
            "sha256=4dc92f738e38fdb788e8b804c7150485c9c8759e6b9098bef303541361c46f97"
        );
    }
}
