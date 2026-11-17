//! Admin passwords and random tokens.

use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;

/// Shortest password accepted for an admin account.
pub const MIN_PASSWORD_LEN: usize = 12;
/// Longest one: argon2 accepts more, but nobody types 1 KB.
pub const MAX_PASSWORD_LEN: usize = 1024;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("the system random generator failed")]
    Random(#[source] getrandom::Error),
    #[error("password hashing failed")]
    Hash,
}

/// Bytes from the OS random generator.
pub fn random_bytes<const N: usize>() -> Result<[u8; N], AuthError> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(AuthError::Random)?;
    Ok(bytes)
}

/// Hash under which a token is stored: whoever reads the database cannot
/// use what they find there.
pub fn token_hash(token: &[u8]) -> [u8; 32] {
    *blake3::hash(token).as_bytes()
}

/// Whether a password is acceptable for a new account.
pub fn password_acceptable(password: &str) -> bool {
    let len = password.chars().count();
    (MIN_PASSWORD_LEN..=MAX_PASSWORD_LEN).contains(&len)
}

/// argon2id with the crate's default parameters (19 MiB, 2 passes), in PHC
/// format. Takes tens of milliseconds: call it off the async threads.
pub fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt: [u8; 16] = random_bytes()?;
    Argon2::default()
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AuthError::Hash)
}

/// Checks a password against a stored hash. A malformed hash never
/// matches.
pub fn verify_password(hash: &str, password: &str) -> bool {
    Argon2::default()
        .verify_password(password.as_bytes(), hash)
        .is_ok()
}

/// Spends the same time as a real check, for a name that does not exist:
/// the response time does not tell whether an account exists.
pub fn waste_a_check(password: &str) {
    // Hash of an unknown random password, computed once.
    static DUMMY: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let dummy = DUMMY.get_or_init(|| hash_password("banc dummy password").ok());
    if let Some(hash) = dummy {
        verify_password(hash, password);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trip() {
        let hash = hash_password("correct horse battery").unwrap();
        assert!(hash.starts_with("$argon2id$"), "{hash}");
        assert!(verify_password(&hash, "correct horse battery"));
        assert!(!verify_password(&hash, "correct horse batterY"));
        assert!(!verify_password("not a hash", "correct horse battery"));
        // Random salt: two hashes of the same password differ.
        assert_ne!(hash, hash_password("correct horse battery").unwrap());
    }

    #[test]
    fn password_length() {
        assert!(!password_acceptable("short"));
        assert!(!password_acceptable(&"x".repeat(MIN_PASSWORD_LEN - 1)));
        assert!(password_acceptable(&"x".repeat(MIN_PASSWORD_LEN)));
        assert!(password_acceptable(&"é".repeat(MIN_PASSWORD_LEN)));
        assert!(!password_acceptable(&"x".repeat(MAX_PASSWORD_LEN + 1)));
    }

    #[test]
    fn tokens_are_random_and_hashed() {
        let a: [u8; 32] = random_bytes().unwrap();
        let b: [u8; 32] = random_bytes().unwrap();
        assert_ne!(a, b);
        assert_eq!(token_hash(&a), token_hash(&a));
        assert_ne!(token_hash(&a), token_hash(&b));
    }
}
