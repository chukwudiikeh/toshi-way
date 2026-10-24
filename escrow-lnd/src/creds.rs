//! Encryption of sellers' node macaroons at rest.
//!
//! A macaroon limited to invoice operations can still create and cancel invoices on a seller's
//! node, so the database never holds one in the clear: it is sealed with AES-256-GCM under a key
//! the operator supplies out of band (`NODE_CREDENTIAL_KEY`, 64 hex characters). A fresh random
//! nonce is stored in front of each ciphertext.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::RngCore;

const NONCE_LEN: usize = 12;

#[derive(Clone)]
pub struct CredentialKey(Aes256Gcm);

impl std::fmt::Debug for CredentialKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CredentialKey(<redacted>)")
    }
}

impl CredentialKey {
    /// Exactly 32 bytes, as 64 hex characters.
    pub fn from_hex(hex_key: &str) -> Result<Self, String> {
        let bytes = hex::decode(hex_key.trim()).map_err(|_| "the key must be hex".to_string())?;
        let bytes: [u8; 32] =
            bytes.try_into().map_err(|_| "the key must be exactly 32 bytes (64 hex characters)".to_string())?;
        Ok(Self(Aes256Gcm::new(&bytes.into())))
    }

    /// A fresh random key, as hex, for the operator to keep.
    pub fn generate_hex() -> String {
        let mut k = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut k);
        hex::encode(k)
    }

    /// Nonce followed by ciphertext.
    pub fn seal(&self, plaintext: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let ct = self.0.encrypt(Nonce::from_slice(&nonce), plaintext).expect("AES-GCM encryption cannot fail");
        let mut out = nonce.to_vec();
        out.extend(ct);
        out
    }

    /// `None` if the data was tampered with, truncated, or sealed under another key.
    pub fn open(&self, sealed: &[u8]) -> Option<Vec<u8>> {
        if sealed.len() <= NONCE_LEN {
            return None;
        }
        let (nonce, ct) = sealed.split_at(NONCE_LEN);
        self.0.decrypt(Nonce::from_slice(nonce), ct).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> CredentialKey {
        CredentialKey::from_hex(&"ab".repeat(32)).unwrap()
    }

    #[test]
    fn round_trips_and_never_stores_the_plaintext() {
        let macaroon = b"0201036c6e6402f801030a10".to_vec();
        let sealed = key().seal(&macaroon);
        assert_eq!(key().open(&sealed), Some(macaroon.clone()));
        assert!(!sealed.windows(macaroon.len()).any(|w| w == macaroon.as_slice()));
    }

    #[test]
    fn each_seal_uses_a_new_nonce() {
        assert_ne!(key().seal(b"same"), key().seal(b"same"));
    }

    #[test]
    fn tampering_truncation_and_the_wrong_key_are_refused() {
        let mut sealed = key().seal(b"secret macaroon");
        assert_eq!(CredentialKey::from_hex(&"cd".repeat(32)).unwrap().open(&sealed), None);
        assert_eq!(key().open(&sealed[..8]), None);
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert_eq!(key().open(&sealed), None);
    }

    #[test]
    fn keys_must_be_32_bytes_of_hex() {
        for bad in ["", "zz", &"ab".repeat(31), &"ab".repeat(33)] {
            assert!(CredentialKey::from_hex(bad).is_err(), "{bad}");
        }
        assert!(CredentialKey::from_hex(&CredentialKey::generate_hex()).is_ok());
    }

    #[test]
    fn debug_output_hides_the_key() {
        assert!(!format!("{:?}", key()).contains("ab"));
    }
}
