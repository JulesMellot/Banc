//! Invitations: the `banc://join` link and the key to copy.
//!
//! Link: `banc://join?u=<server URL>&k=<token>&a=<admin key fingerprint>`.
//! Key to copy, for someone who pastes the URL by hand: `<token>-<fingerprint>`.
//! Either way the workstation learns the server, a single-use token, and the
//! fingerprint of the admin key that every manifest must be signed with.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::keys::Fingerprint;
use crate::text;

/// Start of every invitation link.
pub const LINK_PREFIX: &str = "banc://join?";

/// Single-use enrollment token, 32 random bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct InviteToken([u8; 32]);

impl InviteToken {
    pub const LENGTH: usize = 32;

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for InviteToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&text::base32(&self.0))
    }
}

/// Never prints the token: it would end up in logs.
impl fmt::Debug for InviteToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InviteToken(..)")
    }
}

impl FromStr for InviteToken {
    type Err = InviteError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        text::base32_exact(s).map(Self).ok_or(InviteError::Token)
    }
}

impl Serialize for InviteToken {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for InviteToken {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InviteError {
    #[error("not a Banc invitation link")]
    NotALink,
    #[error("the invitation is missing `{0}`")]
    Missing(&'static str),
    #[error("the invitation repeats `{0}`")]
    Repeated(&'static str),
    #[error("malformed server address")]
    Server,
    #[error("the server address must use https (http is only accepted for this machine)")]
    Insecure,
    #[error("malformed invitation key")]
    Token,
    #[error("malformed admin key fingerprint")]
    Fingerprint,
}

/// Everything a workstation needs to enroll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    /// Base URL of the server, without a trailing slash.
    pub server: String,
    pub token: InviteToken,
    pub fingerprint: Fingerprint,
}

impl Invite {
    /// The `banc://join` link.
    pub fn link(&self) -> String {
        format!(
            "{LINK_PREFIX}u={}&k={}&a={}",
            text::percent_encode(&self.server),
            self.token,
            self.fingerprint
        )
    }

    /// The key to copy, used with the server URL when no link is clicked.
    pub fn key(&self) -> String {
        format!("{}-{}", self.token, self.fingerprint)
    }

    pub fn parse_link(link: &str) -> Result<Self, InviteError> {
        let query = link
            .trim()
            .strip_prefix(LINK_PREFIX)
            .ok_or(InviteError::NotALink)?;
        let (mut server, mut token, mut fingerprint) = (None, None, None);
        for pair in query.split('&') {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let (slot, label) = match name {
                "u" => (&mut server, "u"),
                "k" => (&mut token, "k"),
                "a" => (&mut fingerprint, "a"),
                // Later versions may add parameters.
                _ => continue,
            };
            if slot.replace(value).is_some() {
                return Err(InviteError::Repeated(label));
            }
        }
        let server = server.ok_or(InviteError::Missing("u"))?;
        let server = text::percent_decode(server).ok_or(InviteError::Server)?;
        Ok(Self {
            server: normalize_server(&server)?,
            token: token.ok_or(InviteError::Missing("k"))?.parse()?,
            fingerprint: fingerprint
                .ok_or(InviteError::Missing("a"))?
                .parse()
                .map_err(|_| InviteError::Fingerprint)?,
        })
    }

    /// From a URL and a key pasted by hand.
    pub fn from_server_and_key(server: &str, key: &str) -> Result<Self, InviteError> {
        let (token, fingerprint) = key.trim().split_once('-').ok_or(InviteError::Token)?;
        Ok(Self {
            server: normalize_server(server)?,
            token: token.parse()?,
            fingerprint: fingerprint.parse().map_err(|_| InviteError::Fingerprint)?,
        })
    }
}

/// Checks a server base URL and removes its trailing slashes. Plain http is
/// only accepted for the local machine, for development.
pub fn normalize_server(url: &str) -> Result<String, InviteError> {
    let url = url.trim().trim_end_matches('/');
    let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        return Err(InviteError::Server);
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let host = host_of(authority).ok_or(InviteError::Server)?;
    if host.is_empty()
        || authority.contains('@')
        || rest.contains(['?', '#'])
        || rest.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(InviteError::Server);
    }
    if !secure && !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
        return Err(InviteError::Insecure);
    }
    Ok(url.to_owned())
}

/// Host part of `host[:port]`, IPv6 literals kept in brackets.
fn host_of(authority: &str) -> Option<&str> {
    if authority.starts_with('[') {
        let end = authority.find(']')?;
        let (host, rest) = authority.split_at(end + 1);
        return (rest.is_empty() || valid_port(rest.strip_prefix(':')?)).then_some(host);
    }
    match authority.split_once(':') {
        Some((host, port)) => valid_port(port).then_some(host),
        None => Some(authority),
    }
}

fn valid_port(port: &str) -> bool {
    !port.is_empty() && port.parse::<u16>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::SecretKey;

    fn invite() -> Invite {
        Invite {
            server: "https://banc.example.com".to_owned(),
            token: InviteToken::from_bytes([7; 32]),
            fingerprint: Fingerprint::of(&SecretKey::from_bytes(&[1; 32]).public()),
        }
    }

    #[test]
    fn link_round_trip() {
        let invite = invite();
        let link = invite.link();
        assert!(link.starts_with("banc://join?u=https%3A%2F%2Fbanc.example.com&k="));
        assert_eq!(Invite::parse_link(&link), Ok(invite));
    }

    #[test]
    fn key_round_trip() {
        let invite = invite();
        let parsed = Invite::from_server_and_key("https://banc.example.com/", &invite.key());
        assert_eq!(parsed, Ok(invite.clone()));
        let upper = invite.key().to_ascii_uppercase();
        let parsed = Invite::from_server_and_key(" https://banc.example.com ", &upper);
        assert_eq!(parsed, Ok(invite));
    }

    #[test]
    fn link_parameters_in_any_order_and_unknown_ones_ignored() {
        let invite = invite();
        let link = format!(
            "banc://join?a={}&x=1&k={}&u=https%3A%2F%2Fbanc.example.com",
            invite.fingerprint, invite.token
        );
        assert_eq!(Invite::parse_link(&link), Ok(invite));
    }

    #[test]
    fn link_refusals() {
        let invite = invite();
        let (k, a) = (invite.token.to_string(), invite.fingerprint.to_string());
        let u = "https%3A%2F%2Fbanc.example.com";
        let cases = [
            ("https://banc.example.com".to_owned(), InviteError::NotALink),
            (
                format!("banc://join?k={k}&a={a}"),
                InviteError::Missing("u"),
            ),
            (
                format!("banc://join?u={u}&a={a}"),
                InviteError::Missing("k"),
            ),
            (
                format!("banc://join?u={u}&k={k}"),
                InviteError::Missing("a"),
            ),
            (
                format!("banc://join?u={u}&u={u}&k={k}&a={a}"),
                InviteError::Repeated("u"),
            ),
            (format!("banc://join?u={u}&k=abc&a={a}"), InviteError::Token),
            (
                format!("banc://join?u={u}&k={k}&a=abc"),
                InviteError::Fingerprint,
            ),
            (
                format!("banc://join?u=http%3A%2F%2Fbanc.example.com&k={k}&a={a}"),
                InviteError::Insecure,
            ),
            (
                format!("banc://join?u=%ZZ&k={k}&a={a}"),
                InviteError::Server,
            ),
        ];
        for (link, expected) in cases {
            assert_eq!(Invite::parse_link(&link), Err(expected), "{link}");
        }
    }

    #[test]
    fn server_addresses() {
        for (url, expected) in [
            ("https://banc.example.com", "https://banc.example.com"),
            (
                "https://banc.example.com:8443/",
                "https://banc.example.com:8443",
            ),
            (
                "https://banc.example.com/team",
                "https://banc.example.com/team",
            ),
            ("http://localhost:8080", "http://localhost:8080"),
            ("http://127.0.0.1:8080", "http://127.0.0.1:8080"),
            ("http://[::1]:8080", "http://[::1]:8080"),
            ("https://[2001:db8::1]", "https://[2001:db8::1]"),
        ] {
            assert_eq!(normalize_server(url).as_deref(), Ok(expected), "{url}");
        }
        for (url, expected) in [
            ("banc.example.com", InviteError::Server),
            ("ftp://banc.example.com", InviteError::Server),
            ("https://", InviteError::Server),
            ("https://:443", InviteError::Server),
            ("https://banc.example.com:port", InviteError::Server),
            ("https://user@banc.example.com", InviteError::Server),
            ("https://banc.example.com/?x=1", InviteError::Server),
            ("https://banc example.com", InviteError::Server),
            ("http://banc.example.com", InviteError::Insecure),
            ("http://192.168.1.10", InviteError::Insecure),
        ] {
            assert_eq!(normalize_server(url), Err(expected), "{url}");
        }
    }

    #[test]
    fn token_never_in_debug_output() {
        let invite = invite();
        let debug = format!("{invite:?}");
        assert!(!debug.contains(&invite.token.to_string()), "{debug}");
    }
}
