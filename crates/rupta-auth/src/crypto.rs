//! Keys derived from `RUPTA_SECRET_KEY`, random tokens, hashing and
//! constant-time comparison.
//!
//! One instance secret yields independent subkeys through HKDF-SHA256 with
//! distinct labels, so a key used to sign cookies is never the key that
//! encrypts stored secrets.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Subkeys of the instance secret.
#[derive(Clone)]
pub struct Keys {
    signing: [u8; 32],
    encryption: [u8; 32],
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys(<redacted>)")
    }
}

impl Keys {
    /// Derives the subkeys from the instance secret.
    pub fn derive(secret: &[u8]) -> Keys {
        let hk = hkdf::Hkdf::<Sha256>::new(Some(b"rupta"), secret);
        let mut signing = [0u8; 32];
        let mut encryption = [0u8; 32];
        hk.expand(b"rupta cookie signing v1", &mut signing).expect("32 bytes is a valid HKDF length");
        hk.expand(b"rupta secret encryption v1", &mut encryption).expect("32 bytes is a valid HKDF length");
        Keys { signing, encryption }
    }

    /// Encrypts a secret for storage: base64url(nonce || ciphertext).
    /// `context` binds the ciphertext to where it is stored (e.g.
    /// `sso:42`), so a value copied to another row does not decrypt.
    pub fn encrypt(&self, context: &str, plaintext: &[u8]) -> String {
        let cipher = XChaCha20Poly1305::new((&self.encryption).into());
        let mut nonce = [0u8; 24];
        rand::fill(&mut nonce);
        let aad = chacha20poly1305::aead::Payload { msg: plaintext, aad: context.as_bytes() };
        let ct = cipher.encrypt(&XNonce::from(nonce), aad).expect("encryption does not fail");
        let mut out = nonce.to_vec();
        out.extend_from_slice(&ct);
        URL_SAFE_NO_PAD.encode(out)
    }

    /// Decrypts what [`Keys::encrypt`] produced with the same context.
    pub fn decrypt(&self, context: &str, stored: &str) -> Option<Vec<u8>> {
        let raw = URL_SAFE_NO_PAD.decode(stored).ok()?;
        if raw.len() < 24 + 16 {
            return None;
        }
        let cipher = XChaCha20Poly1305::new((&self.encryption).into());
        let payload = chacha20poly1305::aead::Payload { msg: &raw[24..], aad: context.as_bytes() };
        let nonce = XNonce::try_from(&raw[..24]).ok()?;
        cipher.decrypt(&nonce, payload).ok()
    }

    /// HMAC-SHA256 of `data` with the signing key.
    pub fn mac(&self, data: &[u8]) -> [u8; 32] {
        let mut m = <HmacSha256 as hmac::KeyInit>::new_from_slice(&self.signing).expect("any key length works");
        m.update(data);
        m.finalize().into_bytes().into()
    }

    /// Signs a value: `base64url(value).base64url(mac)`.
    pub fn sign(&self, value: &[u8]) -> String {
        let body = URL_SAFE_NO_PAD.encode(value);
        let mac = self.mac(body.as_bytes());
        format!("{body}.{}", URL_SAFE_NO_PAD.encode(mac))
    }

    /// Verifies a value signed by [`Keys::sign`] and returns it.
    pub fn verify(&self, signed: &str) -> Option<Vec<u8>> {
        let (body, mac) = signed.split_once('.')?;
        let mac = URL_SAFE_NO_PAD.decode(mac).ok()?;
        let want = self.mac(body.as_bytes());
        if !ct_eq(&mac, &want) {
            return None;
        }
        URL_SAFE_NO_PAD.decode(body).ok()
    }
}

/// Constant-time equality of two byte strings.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

/// Hex SHA-256, used to store tokens: a database leak does not reveal
/// usable tokens, and the tokens have enough entropy that a fast hash is
/// sufficient.
pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// `n` random bytes.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    rand::fill(&mut b);
    b
}

/// A random URL-safe token with 32 bytes of entropy.
pub fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<32>())
}

const ALPHANUMERIC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// `n` random characters from `[A-Za-z0-9]`, without modulo bias.
pub fn random_alphanumeric(n: usize) -> String {
    let mut out = String::with_capacity(n);
    while out.len() < n {
        for b in random_bytes::<64>() {
            // 62 * 4 = 248: reject the top values to keep the distribution uniform.
            if b < 248 && out.len() < n {
                out.push(ALPHANUMERIC[(b % 62) as usize] as char);
            }
        }
    }
    out
}

/// base64url without padding.
pub fn b64(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

/// Decodes base64url without padding.
pub fn unb64(data: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(data).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encryption_round_trips_and_is_bound_to_its_context() {
        let keys = Keys::derive(b"0123456789abcdef0123456789abcdef");
        let enc = keys.encrypt("sso:1", b"client-secret");
        assert_eq!(keys.decrypt("sso:1", &enc).unwrap(), b"client-secret");
        assert!(keys.decrypt("sso:2", &enc).is_none());
        assert_ne!(enc, keys.encrypt("sso:1", b"client-secret"), "nonces are random");
        let other = Keys::derive(b"another secret that is long enough!!");
        assert!(other.decrypt("sso:1", &enc).is_none());
    }

    #[test]
    fn signatures_detect_tampering() {
        let keys = Keys::derive(b"0123456789abcdef0123456789abcdef");
        let s = keys.sign(b"hello");
        assert_eq!(keys.verify(&s).unwrap(), b"hello");
        let tampered = format!("{}x", s);
        assert!(keys.verify(&tampered).is_none());
        let swapped = format!("{}.{}", b64(b"evil"), s.split_once('.').unwrap().1);
        assert!(keys.verify(&swapped).is_none());
    }

    #[test]
    fn random_values() {
        let t = random_alphanumeric(40);
        assert_eq!(t.len(), 40);
        assert!(t.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(random_token(), random_token());
        assert!(ct_eq(b"abc", b"abc") && !ct_eq(b"abc", b"abd") && !ct_eq(b"abc", b"ab"));
    }
}
