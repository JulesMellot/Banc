//! The admin key: generated on the admin's workstation, never sent anywhere.
//!
//! It signs the manifest (phase 2). Losing it means a new invitation on every
//! workstation, so Banc has the admin write down a recovery code on paper.
//! The code is the 32-byte secret plus a 2-byte checksum, in base32 with an
//! alphabet that avoids look-alike characters (no I, L, O, U), in groups of
//! five: `7K3QD-X9M2A-…`, 11 groups.

use crate::keys::{Fingerprint, SecretKey};

/// Crockford's base32 alphabet.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const GROUP: usize = 5;
const CHECKSUM: usize = 2;
const PAYLOAD: usize = 32 + CHECKSUM;
/// 34 bytes are 272 bits, 55 characters of 5 bits.
const CHARS: usize = (PAYLOAD * 8).div_ceil(5);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecoveryError {
    #[error("the recovery code has {0} characters instead of {CHARS}")]
    Length(usize),
    #[error("unexpected character `{0}` in the recovery code")]
    Character(char),
    #[error("the recovery code contains a typo (checksum mismatch)")]
    Checksum,
}

/// A new admin key from the OS random generator.
pub fn generate() -> SecretKey {
    SecretKey::generate()
}

/// The recovery code of a key, to write down on paper.
pub fn recovery_code(secret: &SecretKey) -> String {
    let bytes = secret.to_bytes();
    let mut payload = Vec::with_capacity(PAYLOAD);
    payload.extend_from_slice(&bytes);
    payload.extend_from_slice(&checksum(&bytes));
    let chars = encode(&payload);
    chars
        .chunks(GROUP)
        .map(|group| group.iter().map(|&b| char::from(b)).collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

/// The key back from its recovery code. Spaces, dashes and case are
/// ignored; `O` reads as `0`, `I` and `L` as `1`.
pub fn from_recovery_code(code: &str) -> Result<SecretKey, RecoveryError> {
    let mut values = Vec::with_capacity(CHARS);
    for c in code.chars() {
        if c.is_whitespace() || c == '-' {
            continue;
        }
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        let value = ALPHABET
            .iter()
            .position(|&a| char::from(a) == c)
            .ok_or(RecoveryError::Character(c))?;
        values.push(value as u8);
    }
    if values.len() != CHARS {
        return Err(RecoveryError::Length(values.len()));
    }
    let payload = decode(&values);
    let (secret, sum) = payload.split_at(32);
    let mut bytes = [0; 32];
    bytes.copy_from_slice(secret);
    if checksum(&bytes)[..] != sum[..] {
        return Err(RecoveryError::Checksum);
    }
    Ok(SecretKey::from_bytes(&bytes))
}

/// Fingerprint shown next to the code, to check a restored key.
pub fn fingerprint(secret: &SecretKey) -> Fingerprint {
    Fingerprint::of(&secret.public())
}

fn checksum(secret: &[u8; 32]) -> [u8; CHECKSUM] {
    let hash = blake3::derive_key("banc 2026-11 admin key recovery checksum", secret);
    [hash[0], hash[1]]
}

/// Bytes to 5-bit symbols, most significant bits first, last symbol padded
/// with zeros.
fn encode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(CHARS);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize]);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize]);
    }
    out
}

/// 5-bit symbols back to `PAYLOAD` bytes.
fn decode(values: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(PAYLOAD);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &value in values {
        buffer = (buffer << 5) | u32::from(value);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    out.truncate(PAYLOAD);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_round_trip() {
        for seed in [0u8, 1, 0x5a, 0xff] {
            let secret = SecretKey::from_bytes(&[seed; 32]);
            let code = recovery_code(&secret);
            assert_eq!(code.len(), CHARS + CHARS / GROUP - 1, "{code}");
            assert_eq!(code.split('-').count(), 11);
            let back = from_recovery_code(&code).unwrap();
            assert_eq!(back.to_bytes(), secret.to_bytes());
        }
        let secret = generate();
        let back = from_recovery_code(&recovery_code(&secret)).unwrap();
        assert_eq!(back.public(), secret.public());
        assert_eq!(fingerprint(&back), fingerprint(&secret));
    }

    #[test]
    fn code_read_leniently() {
        let secret = SecretKey::from_bytes(&[7; 32]);
        let code = recovery_code(&secret);
        let typed = code.replace('-', " ").to_ascii_lowercase();
        assert_eq!(
            from_recovery_code(&typed).unwrap().to_bytes(),
            secret.to_bytes()
        );
        let confusable = code.replace('0', "O").replace('1', "l");
        assert_eq!(
            from_recovery_code(&confusable).unwrap().to_bytes(),
            secret.to_bytes()
        );
    }

    #[test]
    fn typos_detected() {
        let code = recovery_code(&SecretKey::from_bytes(&[7; 32]));
        let mut chars: Vec<char> = code.chars().collect();
        // Change one character of the secret part.
        chars[3] = if chars[3] == 'A' { 'B' } else { 'A' };
        let typo: String = chars.into_iter().collect();
        assert_eq!(
            from_recovery_code(&typo).unwrap_err(),
            RecoveryError::Checksum
        );
        assert_eq!(
            from_recovery_code("ABC").unwrap_err(),
            RecoveryError::Length(3)
        );
        let bad = code.replacen(|c: char| c.is_ascii_alphanumeric(), "U", 1);
        assert_eq!(
            from_recovery_code(&bad).unwrap_err(),
            RecoveryError::Character('U')
        );
    }

    #[test]
    fn symbol_packing() {
        assert_eq!(encode(&[0xff]), b"ZW");
        assert_eq!(encode(&[0, 0, 0, 0, 0]), b"00000000");
        let payload: Vec<u8> = (0..PAYLOAD as u8).collect();
        let symbols: Vec<u8> = encode(&payload)
            .iter()
            .map(|c| ALPHABET.iter().position(|a| a == c).unwrap() as u8)
            .collect();
        assert_eq!(decode(&symbols), payload);
    }
}
