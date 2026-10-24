//! Arbiters' public keys.
//!
//! An arbiter registers an ECDH P-256 public key (base64 of its SubjectPublicKeyInfo). Buyers
//! encrypt their release secret to it in the browser, so the server only ever stores ciphertext.
//! The server cannot read that ciphertext, so these helpers only check the key has the right
//! shape and make a fingerprint people can compare out of band.

use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};

/// DER prefix of an uncompressed P-256 SubjectPublicKeyInfo; the 65-byte point follows it.
const SPKI_P256_PREFIX: [u8; 26] = [
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48,
    0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
];
const SPKI_LEN: usize = 91;

/// The decoded key bytes, or why the key is not acceptable.
pub fn parse_public_key(b64: &str) -> Result<Vec<u8>, &'static str> {
    let der = STANDARD.decode(b64.trim()).map_err(|_| "the key must be base64")?;
    if der.len() != SPKI_LEN || der[..26] != SPKI_P256_PREFIX || der[26] != 0x04 {
        return Err("the key must be an uncompressed ECDH P-256 public key (SPKI)");
    }
    Ok(der)
}

/// SHA-256 of the key bytes as hex in groups of four: short enough to read aloud or compare by eye.
pub fn fingerprint(b64: &str) -> Option<String> {
    let der = STANDARD.decode(b64.trim()).ok()?;
    let hex = hex::encode(Sha256::digest(der));
    Some(hex.as_bytes().chunks(4).map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(fill: u8) -> String {
        let mut der = SPKI_P256_PREFIX.to_vec();
        der.push(0x04);
        der.extend([fill; 64]);
        STANDARD.encode(der)
    }

    #[test]
    fn accepts_a_p256_spki_and_fingerprints_it() {
        assert!(parse_public_key(&key(7)).is_ok());
        let f = fingerprint(&key(7)).unwrap();
        assert_eq!(f.split(' ').count(), 16);
        assert_ne!(fingerprint(&key(8)).unwrap(), f);
    }

    #[test]
    fn refuses_anything_else() {
        let good = key(7);
        let mut short = STANDARD.decode(&good).unwrap();
        short.pop();
        let mut compressed = STANDARD.decode(&good).unwrap();
        compressed[26] = 0x02;
        let mut wrong_curve = STANDARD.decode(&good).unwrap();
        wrong_curve[13] ^= 1;
        for bad in [
            "".to_string(),
            "not base64!!".to_string(),
            STANDARD.encode(short),
            STANDARD.encode(compressed),
            STANDARD.encode(wrong_curve),
            STANDARD.encode([0u8; 91]),
        ] {
            assert!(parse_public_key(&bad).is_err(), "{bad:.20}");
        }
    }
}
