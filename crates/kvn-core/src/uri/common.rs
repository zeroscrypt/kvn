//! Shared query → model mapping for VLESS/Trojan-style links.

use std::collections::HashMap;

use crate::error::{KvnError, Result};
use crate::profile::{RealityConfig, Security, TcpHeader, TlsConfig, Transport};

use super::{bool01, clean, validate_fingerprint};

/// Map the `type=` transport parameter and its companions.
pub(crate) fn transport_from_query(q: &HashMap<String, String>) -> Result<Transport> {
    let ty = q
        .get("type")
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "tcp".to_owned());
    match ty.as_str() {
        "" | "tcp" => {
            let header_type = match q
                .get("headerType")
                .map(|s| s.trim().to_ascii_lowercase())
                .as_deref()
            {
                None | Some("") | Some("none") => TcpHeader::None,
                Some("http") => TcpHeader::Http,
                Some(other) => return Err(KvnError::Unsupported(format!("headerType `{other}`"))),
            };
            let (path, host) = if header_type == TcpHeader::Http {
                (clean(q.get("path")), clean(q.get("host")))
            } else {
                (None, None)
            };
            Ok(Transport::Tcp {
                header_type,
                path,
                host,
            })
        }
        "ws" => Ok(Transport::Ws {
            path: clean(q.get("path")),
            host: clean(q.get("host")),
            headers: Vec::new(),
        }),
        "grpc" => Ok(Transport::Grpc {
            service_name: clean(q.get("serviceName")).or_else(|| clean(q.get("path"))),
            authority: clean(q.get("authority")),
            multi_mode: q
                .get("mode")
                .map(|m| m.trim().eq_ignore_ascii_case("multi"))
                .unwrap_or(false),
        }),
        "xhttp" | "splithttp" => xhttp_from_query(q),
        "httpupgrade" => Ok(Transport::HttpUpgrade {
            path: clean(q.get("path")),
            host: clean(q.get("host")),
        }),
        "kcp" => Ok(Transport::Kcp {
            header_type: clean(q.get("headerType")),
        }),
        "quic" => Ok(Transport::Quic {
            header_type: clean(q.get("headerType")),
        }),
        other => Err(KvnError::Unsupported(format!("transport `type={other}`"))),
    }
}

/// XHTTP: keep path/host/mode as first-class fields, preserve the `extra`
/// JSON blob verbatim, and keep the flat `x_padding_bytes` companion.
pub(crate) fn xhttp_from_query(q: &HashMap<String, String>) -> Result<Transport> {
    let extra = match clean(q.get("extra")) {
        Some(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(serde_json::Value::Object(map)) => Some(map),
            Ok(_) => return Err(KvnError::invalid("extra", "`extra` is not a JSON object")),
            Err(_) => return Err(KvnError::invalid("extra", "`extra` is not valid JSON")),
        },
        None => None,
    };
    Ok(Transport::Xhttp {
        path: clean(q.get("path")),
        host: clean(q.get("host")),
        mode: clean(q.get("mode")),
        extra,
        x_padding_bytes: clean(q.get("x_padding_bytes")),
        headers: Vec::new(),
    })
}

/// Map `security=` plus TLS/REALITY companions.
pub(crate) fn security_from_query(
    q: &HashMap<String, String>,
    default_tls: bool,
) -> Result<Security> {
    let sec = q
        .get("security")
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_else(|| {
            if default_tls {
                "tls".to_owned()
            } else {
                "none".to_owned()
            }
        });
    match sec.as_str() {
        "" | "none" => Ok(Security::None),
        "tls" => {
            let fingerprint = match clean(q.get("fp")) {
                Some(fp) => Some(validate_fingerprint(&fp)?),
                None => None,
            };
            let alpn = q
                .get("alpn")
                .map(|raw| {
                    raw.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Ok(Security::Tls(TlsConfig {
                sni: clean(q.get("sni")).or_else(|| clean(q.get("peer"))),
                alpn,
                fingerprint,
                allow_insecure: bool01(q.get("allowInsecure")) || bool01(q.get("insecure")),
                pinned_peer_cert_sha256: clean(q.get("pcs"))
                    .or_else(|| clean(q.get("pinnedPeerCertSha256"))),
            }))
        }
        "reality" => {
            let public_key =
                clean(q.get("pbk")).ok_or_else(|| KvnError::MissingField("pbk".into()))?;
            let server_name =
                clean(q.get("sni")).ok_or_else(|| KvnError::MissingField("sni".into()))?;
            let fingerprint = match clean(q.get("fp")) {
                Some(fp) => Some(validate_fingerprint(&fp)?),
                None => None,
            };
            Ok(Security::Reality(RealityConfig {
                public_key,
                server_name,
                short_id: clean(q.get("sid")),
                fingerprint,
                spider_x: clean(q.get("spx")),
                mldsa65_verify: clean(q.get("mldsa65Verify")),
                support_x25519mlkem768: bool01(q.get("support-x25519mlkem768")),
            }))
        }
        other => Err(KvnError::Unsupported(format!("security `{other}`"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn tcp_http_camouflage_keeps_path_host() {
        let t = transport_from_query(&q(&[
            ("type", "tcp"),
            ("headerType", "http"),
            ("path", "/x"),
            ("host", "h"),
        ]))
        .unwrap();
        match t {
            Transport::Tcp {
                header_type: TcpHeader::Http,
                path,
                host,
            } => {
                assert_eq!(path.as_deref(), Some("/x"));
                assert_eq!(host.as_deref(), Some("h"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn tcp_none_drops_stray_path() {
        let t = transport_from_query(&q(&[("path", "/ignored")])).unwrap();
        match t {
            Transport::Tcp { path, host, .. } => {
                assert!(path.is_none() && host.is_none());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn tls_allow_insecure_from_both_spellings() {
        let s =
            security_from_query(&q(&[("security", "tls"), ("allowInsecure", "1")]), false).unwrap();
        match s {
            Security::Tls(t) => assert!(t.allow_insecure),
            other => panic!("{other:?}"),
        }
        let s =
            security_from_query(&q(&[("security", "tls"), ("insecure", "true")]), false).unwrap();
        match s {
            Security::Tls(t) => assert!(t.allow_insecure),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn trojan_defaults_to_tls_when_security_absent() {
        let s = security_from_query(&q(&[]), true).unwrap();
        assert!(matches!(s, Security::Tls(_)));
        let s = security_from_query(&q(&[("security", "none")]), true).unwrap();
        assert!(matches!(s, Security::None));
    }

    #[test]
    fn xhttp_bad_extra_is_rejected() {
        assert!(transport_from_query(&q(&[("type", "xhttp"), ("extra", "{oops")])).is_err());
        assert!(transport_from_query(&q(&[("type", "xhttp"), ("extra", "[1]")])).is_err());
    }
}
