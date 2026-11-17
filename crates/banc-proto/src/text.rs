//! Text encodings used in links, keys and JSON.

use data_encoding::{BASE32_NOPAD, HEXLOWER};

/// Lowercase base32 without padding: short, case-insensitive, no character
/// that needs escaping in a URL.
pub(crate) fn base32(bytes: &[u8]) -> String {
    BASE32_NOPAD.encode(bytes).to_ascii_lowercase()
}

/// Decodes base32 in either case into exactly `N` bytes.
pub(crate) fn base32_exact<const N: usize>(text: &str) -> Option<[u8; N]> {
    let bytes = BASE32_NOPAD
        .decode(text.trim().to_ascii_uppercase().as_bytes())
        .ok()?;
    bytes.try_into().ok()
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    HEXLOWER.encode(bytes)
}

pub(crate) fn hex_exact<const N: usize>(text: &str) -> Option<[u8; N]> {
    let bytes = HEXLOWER.decode(text.as_bytes()).ok()?;
    bytes.try_into().ok()
}

/// Percent-encodes everything except the unreserved characters of RFC 3986.
pub(crate) fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Decodes `%XX` sequences. `None` if a sequence is malformed or the result
/// is not UTF-8.
pub(crate) fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let pair = bytes.get(i + 1..i + 3)?;
            let pair = std::str::from_utf8(pair).ok()?;
            out.push(u8::from_str_radix(pair, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_round_trip_in_both_cases() {
        let text = base32(&[1, 2, 3, 4, 5]);
        assert_eq!(text, text.to_ascii_lowercase());
        assert_eq!(base32_exact::<5>(&text), Some([1, 2, 3, 4, 5]));
        assert_eq!(
            base32_exact::<5>(&text.to_ascii_uppercase()),
            Some([1, 2, 3, 4, 5])
        );
        assert_eq!(base32_exact::<4>(&text), None);
        assert_eq!(base32_exact::<5>("not base32!"), None);
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(hex(&[0xab, 0x01]), "ab01");
        assert_eq!(hex_exact::<2>("ab01"), Some([0xab, 0x01]));
        assert_eq!(hex_exact::<2>("ab0"), None);
        assert_eq!(hex_exact::<1>("ab01"), None);
    }

    #[test]
    fn percent_round_trip() {
        let url = "https://banc.example.com:8443/équipe?a=b&c";
        let encoded = percent_encode(url);
        assert!(!encoded.contains(['/', ':', '?', '&', '=']), "{encoded}");
        assert_eq!(percent_decode(&encoded).as_deref(), Some(url));
    }

    #[test]
    fn percent_decode_refuses_malformed_text() {
        assert_eq!(percent_decode("%"), None);
        assert_eq!(percent_decode("%4"), None);
        assert_eq!(percent_decode("%zz"), None);
        assert_eq!(percent_decode("%FF"), None);
        assert_eq!(percent_decode("plain").as_deref(), Some("plain"));
    }
}
