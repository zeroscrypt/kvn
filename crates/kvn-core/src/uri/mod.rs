//! Share-link parsing: VLESS, VMess, Trojan, Shadowsocks.
//!
//! The parsers implement the de-facto formats emitted by 3x-ui, v2rayN,
//! v2rayNG and friends (verified against 3x-ui `internal/sub/service.go`
//! and Xray share-link conventions), plus subscription payloads:
//! newline-separated link lists, optionally wrapped in base64.
//!
//! Design rules:
//! - Anything we cannot faithfully represent in the engine config is a
//!   *typed error*, never a silent drop.
//! - Everything the engine can consume is preserved, including XHTTP
//!   `extra` blobs verbatim.
//! - Errors never contain credentials.

mod common;
mod ss;
mod trojan;
mod vless;
mod vmess;

use std::collections::HashMap;

use percent_encoding::percent_decode_str;

use crate::error::{KvnError, Result};
use crate::profile::{Profile, ProfileSource};

/// A single import issue; `input` is always redacted.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportIssue {
    pub input: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportReport {
    pub profiles: Vec<Profile>,
    pub issues: Vec<ImportIssue>,
}

impl ImportReport {
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}

const SCHEMES: [&str; 4] = ["vless", "vmess", "trojan", "ss"];

fn scheme_of(uri: &str) -> Option<String> {
    let (scheme, _) = uri.split_once("://")?;
    let scheme = scheme.trim().to_ascii_lowercase();
    if SCHEMES.contains(&scheme.as_str()) {
        Some(scheme)
    } else if scheme.is_empty() {
        None
    } else {
        Some(scheme) // dispatch will reject with UnsupportedScheme
    }
}

/// Parse a single share link.
pub fn parse_uri(uri: &str) -> Result<Profile> {
    let trimmed = uri.trim();
    let scheme = scheme_of(trimmed).ok_or(KvnError::UnrecognizedInput)?;
    let profile = match scheme.as_str() {
        "vless" => vless::parse(trimmed)?,
        "vmess" => vmess::parse(trimmed)?,
        "trojan" => trojan::parse(trimmed)?,
        "ss" => ss::parse(trimmed)?,
        other => return Err(KvnError::UnsupportedScheme(other.to_owned())),
    };
    Ok(profile)
}

/// Parse an import payload: one link, many newline-separated links, or a
/// base64-encoded subscription body containing links. Duplicates within the
/// payload are collapsed.
pub fn parse_import_payload(payload: &str, source: ProfileSource) -> ImportReport {
    let mut report = ImportReport::default();
    let text = payload.trim();
    if text.is_empty() {
        report.issues.push(ImportIssue {
            input: String::new(),
            message: KvnError::UnrecognizedInput.to_string(),
        });
        return report;
    }

    let body: String = if text.contains("://") {
        text.to_owned()
    } else {
        match decode_base64(text) {
            Some(bytes) => match String::from_utf8(bytes) {
                Ok(decoded) if decoded.contains("://") => decoded,
                _ => {
                    report.issues.push(ImportIssue {
                        input: preview(text),
                        message: KvnError::UnrecognizedInput.to_string(),
                    });
                    return report;
                }
            },
            None => {
                report.issues.push(ImportIssue {
                    input: preview(text),
                    message: KvnError::UnrecognizedInput.to_string(),
                });
                return report;
            }
        }
    };

    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_uri(line) {
            Ok(mut profile) => {
                profile.source = source.clone();
                if profile.name.trim().is_empty() {
                    profile.name = format!(
                        "{} {}",
                        profile.protocol().as_str(),
                        profile.endpoint.label()
                    );
                }
                if report.profiles.iter().any(|p| p.same_server_as(&profile)) {
                    continue; // duplicate within payload
                }
                report.profiles.push(profile);
            }
            Err(err) => report.issues.push(ImportIssue {
                input: preview(line),
                message: err.to_string(),
            }),
        }
    }
    report
}

fn preview(line: &str) -> String {
    let redacted = crate::secret::redact_link(line);
    if redacted.chars().count() > 80 {
        let cut: String = redacted.chars().take(77).collect();
        format!("{cut}...")
    } else {
        redacted
    }
}

/// Lenient base64 decoding: accepts standard and URL-safe alphabets, with
/// or without padding, ignoring whitespace (subscription bodies often wrap).
pub(crate) fn decode_base64(input: &str) -> Option<Vec<u8>> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    use base64::Engine;

    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.is_empty() {
        return None;
    }
    for engine in [&STANDARD, &STANDARD_NO_PAD, &URL_SAFE, &URL_SAFE_NO_PAD] {
        if let Ok(bytes) = engine.decode(&cleaned) {
            return Some(bytes);
        }
    }
    None
}

/// Percent-decode a fragment-style component (`+` stays literal).
pub(crate) fn percent_decode(s: &str) -> String {
    percent_decode_str(s).decode_utf8_lossy().into_owned()
}

/// Form-decode a query value the way Go's `url.Values.Encode()` pairs with
/// it: `+` means space, then percent-decoding.
pub(crate) fn form_decode(s: &str) -> String {
    let plus_decoded = s.replace('+', " ");
    percent_decode(&plus_decoded)
}

/// Split `host:port`, accepting bracketed IPv6 literals.
pub(crate) fn parse_hostport(input: &str) -> Result<(String, u16)> {
    let input = input.trim();
    if input.is_empty() {
        return Err(KvnError::MissingField("address".into()));
    }
    let (host, port_str) = if let Some(rest) = input.strip_prefix('[') {
        let close = rest
            .find(']')
            .ok_or_else(|| KvnError::MalformedLink("unclosed IPv6 bracket".into()))?;
        let host = &rest[..close];
        let after = &rest[close + 1..];
        let port = after
            .strip_prefix(':')
            .ok_or_else(|| KvnError::MissingField("port".into()))?;
        (host.to_owned(), port)
    } else {
        let (host, port) = input
            .rsplit_once(':')
            .ok_or_else(|| KvnError::MissingField("port".into()))?;
        if host.contains(':') {
            return Err(KvnError::MalformedLink(
                "IPv6 addresses must be bracketed, e.g. [2001:db8::1]:443".into(),
            ));
        }
        (host.to_owned(), port)
    };
    if host.is_empty() {
        return Err(KvnError::MissingField("address".into()));
    }
    if host.chars().any(|c| c.is_whitespace()) {
        return Err(KvnError::invalid("address", "contains whitespace"));
    }
    let port: u16 = port_str
        .trim()
        .parse()
        .map_err(|_| KvnError::InvalidPort(port_str.trim().to_owned()))?;
    if port == 0 {
        return Err(KvnError::InvalidPort("0".into()));
    }
    Ok((host, port))
}

/// Structural pieces of a `scheme://userinfo@host:port?query#fragment` link.
#[derive(Debug)]
pub(crate) struct RawLink {
    pub userinfo: Option<String>,
    pub host: String,
    pub port: u16,
    pub query: HashMap<String, String>,
    pub fragment: Option<String>,
}

impl RawLink {
    pub fn parse(uri: &str) -> Result<Self> {
        let uri = uri.trim();
        let (scheme, rest) = uri
            .split_once("://")
            .ok_or_else(|| KvnError::MalformedLink("missing scheme".into()))?;
        let _ = scheme.trim().to_ascii_lowercase();

        let (before_frag, frag) = match rest.split_once('#') {
            Some((a, b)) => (a, Some(b)),
            None => (rest, None),
        };
        let (authority, query_raw) = match before_frag.split_once('?') {
            Some((a, b)) => (a, Some(b)),
            None => (before_frag, None),
        };
        if authority.is_empty() {
            return Err(KvnError::MissingField("address".into()));
        }

        let (userinfo, hostport) = match authority.rsplit_once('@') {
            Some((u, hp)) => (Some(percent_decode(u)), hp),
            None => (None, authority),
        };
        let (host, port) = parse_hostport(hostport)?;

        let mut query = HashMap::new();
        if let Some(q) = query_raw {
            for pair in q.split('&') {
                if pair.is_empty() {
                    continue;
                }
                let (k, v) = match pair.split_once('=') {
                    Some((k, v)) => (k, v),
                    None => (pair, ""),
                };
                query.insert(form_decode(k), form_decode(v));
            }
        }

        Ok(RawLink {
            userinfo,
            host,
            port,
            query,
            fragment: frag.map(percent_decode),
        })
    }
}

pub(crate) fn clean(value: Option<&String>) -> Option<String> {
    value
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(|v| v.to_owned())
}

pub(crate) fn bool01(value: Option<&String>) -> bool {
    match value.map(|v| v.trim().to_ascii_lowercase()) {
        Some(v) => matches!(v.as_str(), "1" | "true" | "yes" | "on"),
        None => false,
    }
}

/// uTLS fingerprints accepted by the engine. A strict list keeps typos from
/// turning into confusing engine errors at connect time.
pub(crate) const VALID_FINGERPRINTS: &[&str] = &[
    "chrome",
    "firefox",
    "safari",
    "ios",
    "android",
    "edge",
    "360",
    "qq",
    "random",
    "randomized",
];

pub(crate) fn validate_fingerprint(fp: &str) -> Result<String> {
    let fp = fp.trim().to_ascii_lowercase();
    if VALID_FINGERPRINTS.contains(&fp.as_str()) {
        Ok(fp)
    } else {
        Err(KvnError::invalid(
            "fp",
            format!(
                "unknown uTLS fingerprint `{fp}` (supported: {})",
                VALID_FINGERPRINTS.join(", ")
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostport_ipv4() {
        assert_eq!(
            parse_hostport("example.com:443").unwrap(),
            ("example.com".into(), 443)
        );
    }

    #[test]
    fn hostport_ipv6_bracketed() {
        assert_eq!(
            parse_hostport("[2001:db8::1]:8443").unwrap(),
            ("2001:db8::1".into(), 8443)
        );
    }

    #[test]
    fn hostport_rejects_bare_ipv6_and_bad_port() {
        assert!(parse_hostport("2001:db8::1").is_err());
        assert!(parse_hostport("host:0").is_err());
        assert!(parse_hostport("host:99999").is_err());
        assert!(parse_hostport("host").is_err());
    }

    #[test]
    fn raw_link_decodes_query_like_go() {
        let link = RawLink::parse(
            "vless://abc@example.com:443?path=%2Ftest&remark=US+East&extra=%7B%22a%22%3A1%7D#My%20Server",
        )
        .unwrap();
        assert_eq!(link.query.get("path").unwrap(), "/test");
        assert_eq!(link.query.get("remark").unwrap(), "US East");
        assert_eq!(link.query.get("extra").unwrap(), r#"{"a":1}"#);
        assert_eq!(link.fragment.as_deref(), Some("My Server"));
    }

    #[test]
    fn import_payload_parses_base64_body() {
        use base64::Engine;
        let links = "vless://uuid-1@a.example:443?type=tcp&security=none#One\nvless://uuid-2@b.example:443?type=tcp&security=none#Two";
        let body = base64::engine::general_purpose::STANDARD.encode(links);
        let report = parse_import_payload(&body, ProfileSource::UriImport);
        assert_eq!(report.profiles.len(), 2);
        assert!(report.issues.is_empty());
        assert_eq!(report.profiles[0].name, "One");
    }

    #[test]
    fn import_payload_deduplicates() {
        let link = "vless://uuid-1@a.example:443?type=tcp&security=none#One";
        let payload = format!("{link}\n{link}");
        let report = parse_import_payload(&payload, ProfileSource::UriImport);
        assert_eq!(report.profiles.len(), 1);
    }

    #[test]
    fn import_payload_reports_issues_with_redacted_preview() {
        let report = parse_import_payload(
            "vless://secret-uuid@host.example:443?security=bogus",
            ProfileSource::UriImport,
        );
        assert!(report.profiles.is_empty());
        assert_eq!(report.issues.len(), 1);
        assert!(!report.issues[0].input.contains("secret-uuid"));
        assert!(report.issues[0]
            .input
            .starts_with("vless://***@host.example:443"));
    }

    #[test]
    fn unsupported_scheme_is_explicit() {
        let err = parse_uri("wireguard://key@host:51820").unwrap_err();
        assert!(matches!(err, KvnError::UnsupportedScheme(ref s) if s == "wireguard"));
    }
}
