//! Secret string wrapper and redaction helpers.
//!
//! `Secret` serializes transparently (the local store needs the real value)
//! but renders as `***` through `Debug`/`Display`, so credentials cannot
//! leak into logs, panic messages or UI by accident. The real value is only
//! reachable through the explicitly named [`Secret::expose`] method — so
//! `grep -rn "expose()"` is a complete audit of every place secrets are
//! read.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Secret(value.into())
    }

    /// Explicit access to the raw value.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Secret {}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Secret(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Secret(value.to_owned())
    }
}

/// Redact a share link for logs and diagnostics: keeps the scheme and the
/// server `host:port`, drops every piece of secret material (credentials
/// in userinfo, VMess base64 payload, query values that carry keys).
pub fn redact_link(link: &str) -> String {
    let trimmed = link.trim();
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return "***".to_string();
    };
    let scheme = scheme.to_ascii_lowercase();
    if scheme == "vmess" {
        // The whole payload is the credential.
        return "vmess://***".to_string();
    }
    let rest = rest.split('#').next().unwrap_or(rest);
    let authority = rest.split('?').next().unwrap_or(rest);
    match authority.rsplit_once('@') {
        Some((_userinfo, hostport)) => format!("{scheme}://***@{hostport}"),
        None => format!("{scheme}://{authority}"),
    }
}

/// Redact a subscription URL: keeps scheme + host, drops path and query
/// (subscription paths are secrets — they grant access to configs).
pub fn redact_url(url: &str) -> String {
    let trimmed = url.trim();
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return "***".to_string();
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    format!("{scheme}://{host}/***")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_and_display_are_redacted() {
        let s = Secret::new("c0ffee-uuid");
        assert_eq!(format!("{s:?}"), "Secret(***)");
        assert_eq!(format!("{s}"), "***");
        assert_eq!(s.expose(), "c0ffee-uuid");
    }

    #[test]
    fn secret_serializes_transparently() {
        let s = Secret::new("token");
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"token\"");
        let back: Secret = serde_json::from_str(&json).unwrap();
        assert_eq!(back.expose(), "token");
    }

    #[test]
    fn redact_link_keeps_host_drops_credentials() {
        let link = "vless://2f5e6f5c-1111-2222-3333-444455556666@vpn.example.com:443?pbk=KEY&sid=abcd#Main";
        let red = redact_link(link);
        assert_eq!(red, "vless://***@vpn.example.com:443");
        assert!(!red.contains("2f5e6f5c"));
        assert!(!red.contains("KEY"));
    }

    #[test]
    fn redact_vmess_hides_payload() {
        assert_eq!(
            redact_link("vmess://eyJhZGQiOiJleGFtcGxlIn0="),
            "vmess://***"
        );
    }

    #[test]
    fn redact_trojan_password() {
        let red = redact_link("trojan://secret-pass@host.name:8443?sni=x#Work");
        assert_eq!(red, "trojan://***@host.name:8443");
    }

    #[test]
    fn redact_url_keeps_only_origin() {
        assert_eq!(
            redact_url("https://sub.example.com:2096/sub/AbCdEf123?token=x"),
            "https://sub.example.com:2096/***"
        );
        assert_eq!(redact_url("not a url"), "***");
    }
}
