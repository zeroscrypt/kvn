//! Shadowsocks link parsing: SIP002 (`ss://b64(method:password)@host:port`)
//! and the legacy fully-base64 form, plus Shadowsocks-2022 percent-encoded
//! userinfo. SS plugins are explicitly unsupported by Xray-core and are
//! rejected with a clear error instead of silently connecting as plain SS.

use crate::error::{KvnError, Result};
use crate::profile::{
    Endpoint, Profile, ProfileSource, ProtocolSpec, Security, TcpHeader, Transport,
};
use crate::secret::Secret;

use super::{decode_base64, parse_hostport, percent_decode};

/// Ciphers supported by Xray-core. Unknown values are rejected so a typo
/// cannot silently downgrade to a different method.
const SS_METHODS: [&str; 10] = [
    "aes-128-gcm",
    "aes-256-gcm",
    "chacha20-ietf-poly1305",
    "chacha20-poly1305",
    "xchacha20-ietf-poly1305",
    "none",
    "plain",
    "2022-blake3-aes-128-gcm",
    "2022-blake3-aes-256-gcm",
    "2022-blake3-chacha20-poly1305",
];

pub(crate) fn parse(uri: &str) -> Result<Profile> {
    let rest = uri
        .trim()
        .split_once("://")
        .map(|(_, r)| r)
        .ok_or_else(|| KvnError::MalformedLink("missing scheme".into()))?;

    let (before_frag, frag) = match rest.split_once('#') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    let (main, query) = match before_frag.split_once('?') {
        Some((a, b)) => (a, Some(b)),
        None => (before_frag, None),
    };
    if main.is_empty() {
        return Err(KvnError::MalformedLink("empty shadowsocks payload".into()));
    }

    let (method, password, host, port) = if let Some((userinfo, hostport)) = main.rsplit_once('@') {
        // SIP002 form.
        let (method, password) = decode_userinfo(userinfo)?;
        let (host, port) = parse_hostport(hostport)?;
        (method, password, host, port)
    } else {
        // Legacy form: base64("method:password@host:port").
        let bytes = decode_base64(main).ok_or_else(|| {
            KvnError::MalformedLink("legacy ss payload is not valid base64".into())
        })?;
        let text = String::from_utf8(bytes)
            .map_err(|_| KvnError::MalformedLink("legacy ss payload is not UTF-8".into()))?;
        let (creds, hostport) = text
            .rsplit_once('@')
            .ok_or_else(|| KvnError::MalformedLink("legacy ss payload has no host part".into()))?;
        let (method, password) = creds.split_once(':').ok_or_else(|| {
            KvnError::MalformedLink("legacy ss payload has no method:password".into())
        })?;
        let (host, port) = parse_hostport(hostport)?;
        (method.to_owned(), password.to_owned(), host, port)
    };

    if let Some(query) = query {
        for pair in query.split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            if k.eq_ignore_ascii_case("plugin") && !v.trim().is_empty() {
                let plugin_name = percent_decode(v)
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_owned();
                return Err(KvnError::Unsupported(format!(
                    "Shadowsocks plugin `{plugin_name}` (Xray-core does not support ss plugins)"
                )));
            }
        }
    }

    let method_trimmed = method.trim().to_ascii_lowercase();
    if !SS_METHODS.contains(&method_trimmed.as_str()) {
        return Err(KvnError::Unsupported(format!(
            "shadowsocks method `{method}` (supported by the engine: {})",
            SS_METHODS.join(", ")
        )));
    }
    if password.is_empty() {
        return Err(KvnError::MissingField("password".into()));
    }

    Ok(Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name: frag.map(percent_decode).unwrap_or_default(),
        source: ProfileSource::UriImport,
        endpoint: Endpoint {
            address: host,
            port,
        },
        protocol: ProtocolSpec::Shadowsocks {
            method: method_trimmed,
            password: Secret::new(password),
        },
        transport: Transport::Tcp {
            header_type: TcpHeader::None,
            path: None,
            host: None,
        },
        security: Security::None,
    })
}

/// Decode `method:password` userinfo: percent-encoded form first
/// (Shadowsocks-2022 convention), then base64 form (SIP002 classic).
fn decode_userinfo(userinfo: &str) -> Result<(String, String)> {
    let percent_form = percent_decode(userinfo);
    if let Some((m, p)) = percent_form.split_once(':') {
        return Ok((m.to_owned(), p.to_owned()));
    }
    if let Some(bytes) = decode_base64(userinfo) {
        if let Ok(text) = String::from_utf8(bytes) {
            if let Some((m, p)) = text.split_once(':') {
                return Ok((m.to_owned(), p.to_owned()));
            }
        }
    }
    Err(KvnError::MalformedLink(
        "ss userinfo is neither `method:password` nor base64 `method:password`".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn sip002_base64_userinfo() {
        let userinfo = base64::engine::general_purpose::STANDARD.encode("aes-256-gcm:pass123");
        let uri = format!("ss://{userinfo}@s.example.com:8388#SS-One");
        let p = parse(&uri).unwrap();
        assert_eq!(p.name, "SS-One");
        assert_eq!(
            p.endpoint,
            Endpoint {
                address: "s.example.com".into(),
                port: 8388
            }
        );
        match &p.protocol {
            ProtocolSpec::Shadowsocks { method, password } => {
                assert_eq!(method, "aes-256-gcm");
                assert_eq!(password.expose(), "pass123");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn sip002_percent_userinfo_2022() {
        // Shadowsocks-2022: password is base64 with `+` and `/`, encoded in
        // the userinfo as %2B / %2F.
        let uri = "ss://2022-blake3-aes-256-gcm:p%2Bss%2Fwd%3D@h.example.com:443#SS2022";
        let p = parse(uri).unwrap();
        match &p.protocol {
            ProtocolSpec::Shadowsocks { method, password } => {
                assert_eq!(method, "2022-blake3-aes-256-gcm");
                assert_eq!(password.expose(), "p+ss/wd=");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn legacy_fully_base64_form() {
        let payload = base64::engine::general_purpose::STANDARD
            .encode("chacha20-ietf-poly1305:secret@l.example.com:9000");
        let uri = format!("ss://{payload}#Legacy");
        let p = parse(&uri).unwrap();
        assert_eq!(p.name, "Legacy");
        assert_eq!(p.endpoint.port, 9000);
        match &p.protocol {
            ProtocolSpec::Shadowsocks { method, .. } => {
                assert_eq!(method, "chacha20-ietf-poly1305")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn plugin_is_rejected_with_name() {
        let userinfo = base64::engine::general_purpose::STANDARD.encode("aes-256-gcm:p");
        let uri = format!("ss://{userinfo}@h.example.com:443?plugin=obfs-local%3Bobfs%3Dhttp");
        let err = parse(&uri).unwrap_err();
        assert!(matches!(err, KvnError::Unsupported(ref m) if m.contains("obfs-local")));
    }

    #[test]
    fn unknown_method_is_rejected() {
        let userinfo = base64::engine::general_purpose::STANDARD.encode("aes-128-cfb:p");
        let uri = format!("ss://{userinfo}@h.example.com:443");
        assert!(matches!(parse(&uri).unwrap_err(), KvnError::Unsupported(_)));
    }
}
