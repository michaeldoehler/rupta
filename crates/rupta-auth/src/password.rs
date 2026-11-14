//! Password hashing with Argon2id.
//!
//! Parameters follow the OWASP recommendation (19 MiB memory, 2 passes,
//! 1 lane). Hashes are PHC strings, so parameters can be raised later
//! without invalidating existing hashes.

use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, Version};

/// Shortest password accepted for local accounts.
pub const MIN_PASSWORD_LEN: usize = 10;
/// Longest password accepted (hashing cost grows with length).
pub const MAX_PASSWORD_LEN: usize = 256;

fn argon2() -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::new(19 * 1024, 2, 1, None).expect("valid parameters"))
}

/// Hashes a password. CPU-heavy: call it off the async runtime.
pub fn hash(password: &str) -> String {
    argon2().hash_password(password.as_bytes()).expect("hashing with valid parameters does not fail").to_string()
}

/// Checks a password against a stored hash. CPU-heavy.
pub fn verify(password: &str, stored: &str) -> bool {
    match PasswordHash::new(stored) {
        Ok(parsed) => argon2().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

/// Spends the time of a verification without a stored hash, so that a
/// login for an unknown email takes as long as one with a wrong password.
/// The dummy hash is of a random password, so nothing can match it.
pub fn verify_dummy(password: &str) {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let stored = DUMMY.get_or_init(|| hash(&crate::crypto::random_token()));
    let _ = verify(password, stored);
}

/// Checks the password policy; returns the reason it is refused.
pub fn check_policy(password: &str) -> Result<(), String> {
    let n = password.chars().count();
    if n < MIN_PASSWORD_LEN {
        return Err(format!("the password must have at least {MIN_PASSWORD_LEN} characters"));
    }
    if password.len() > MAX_PASSWORD_LEN {
        return Err(format!("the password must have at most {MAX_PASSWORD_LEN} bytes"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_verify_and_are_argon2id() {
        let h = hash("correct horse battery");
        assert!(h.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{h}");
        assert!(verify("correct horse battery", &h));
        assert!(!verify("Correct horse battery", &h));
        assert!(!verify("x", "not a hash"));
        assert_ne!(h, hash("correct horse battery"), "salted");
    }

    #[test]
    fn policy() {
        assert!(check_policy("short").is_err());
        assert!(check_policy("long enough pw").is_ok());
        assert!(check_policy(&"x".repeat(300)).is_err());
    }
}
