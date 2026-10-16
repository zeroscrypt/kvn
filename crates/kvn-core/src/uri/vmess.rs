//! VMess link parsing (base64-encoded JSON, v2rayN layout).
//!
//! 3x-ui flattens XHTTP `extra` fields directly onto the JSON object plus a
//! flat `x_padding_bytes` key; both are collected back into the profile.

use serde_json::{Map, Value};

use crate::error::{KvnError, Result};
use crate::profile::{
    Endpoint, Header, Profile, ProfileSource, ProtocolSpec, Security, TcpHeader, TlsConfig,
    Transport,
};
use crate::secret::Secret;

use super::{clean, decode_base64, validate_fingerprint};

const VMESS_CIPHERS: [&str; 5] = ["auto", "none", "zero", "aes-128-gcm", "chacha20-poly1305"];

/// XHTTP keys 3x-ui flattens onto the VMess object (bidirectional fields of
/// Xray's SplitHTTPConfig).
const XHTTP_EXTRA_KEYS: [&str; 24] = [
    "xPaddingBytes",
    "xPaddingObfsMode",
    "xPaddingKey",
    "xPaddingHeader",
    "xPaddingPlacement",
    "xPaddingMethod",
    "uplinkHTTPMethod",
    "sessionIDPlacement",
    "sessionIDKey",
    "sessionIDTable",
    "sessionIDLength",
    "seqPlacement",
    "seqKey",
    "uplinkDataPlacement",
    "uplinkDataKey",
    "scMaxEachPostBytes",
    "scMinPostsIntervalMs",
    "scMaxBufferedPosts",
    "scStreamUpServerSecs",
    "xmux",
    "noGRPCHeader",
    "noSSEHeader",
    "serverMaxHeaderBytes",
    "uplinkChunkSize",
];

pub(crate) fn parse(uri: &str) -> Result<Profile> {
    let rest = uri
        .trim()
        .split_once("://")
        .map(|(_, r)| r)
        .ok_or_else(|| KvnError::MalformedLink("missing scheme".into()))?;
    // Some clients append `#name` after the payload.
    let payload = rest.split('#').next().unwrap_or(rest);
    let bytes = decode_base64(payload)
        .ok_or_else(|| KvnError::MalformedLink("vmess payload is not valid base64".into()))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| KvnError::MalformedLink("vmess payload is not UTF-8".into()))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| KvnError::MalformedLink("vmess payload is not valid JSON".into()))?;
    let obj = value
        .as_object()
        .ok_or_else(|| KvnError::MalformedLink("vmess payload is not a JSON object".into()))?;

    let get_str = |key: &str| -> Option<String> {
        match obj.get(key) {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Number(n)) => Some(n.to_string()),
            Some(Value::Bool(b)) => Some(b.to_string()),
            _ => None,
        }
    };

    let address = get_str("add")
        .map(|a| a.trim().to_owned())
        .filter(|a| !a.is_empty())
        .ok_or_else(|| KvnError::MissingField("add".into()))?;
    if address.chars().any(|c| c.is_whitespace()) {
        return Err(KvnError::invalid("add", "contains whitespace"));
    }
    let port_raw = get_str("port").ok_or_else(|| KvnError::MissingField("port".into()))?;
    let port: u16 = port_raw
        .trim()
        .parse()
        .map_err(|_| KvnError::InvalidPort(port_raw.trim().to_owned()))?;
    if port == 0 {
        return Err(KvnError::InvalidPort("0".into()));
    }
    let uuid = get_str("id")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| KvnError::MissingField("id".into()))?;
    if uuid.len() > 64 {
        return Err(KvnError::invalid("id", "longer than 64 bytes"));
    }
    let alter_id: u16 = get_str("aid")
        .unwrap_or_else(|| "0".into())
        .trim()
        .parse()
        .unwrap_or(0);
    let cipher = clean(get_str("scy").as_ref()).unwrap_or_else(|| "auto".into());
    if !VMESS_CIPHERS.contains(&cipher.as_str()) {
        return Err(KvnError::Unsupported(format!("vmess cipher `{cipher}`")));
    }

    let net = get_str("net")
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "tcp".into());
    let header_type = get_str("type").map(|s| s.trim().to_ascii_lowercase());
    let path = clean(get_str("path").as_ref());
    let host = clean(get_str("host").as_ref());

    let transport = match net.as_str() {
        "" | "tcp" => {
            let ht = match header_type.as_deref() {
                None | Some("") | Some("none") => TcpHeader::None,
                Some("http") => TcpHeader::Http,
                Some(other) => {
                    return Err(KvnError::Unsupported(format!(
                        "vmess header `type={other}`"
                    )))
                }
            };
            let (p, h) = if ht == TcpHeader::Http {
                (path, host)
            } else {
                (None, None)
            };
            Transport::Tcp {
                header_type: ht,
                path: p,
                host: h,
            }
        }
        "ws" => Transport::Ws {
            path,
            host,
            headers: Vec::new(),
        },
        "grpc" => Transport::Grpc {
            service_name: path,
            authority: clean(get_str("authority").as_ref()),
            multi_mode: header_type.as_deref() == Some("multi"),
        },
        "xhttp" | "splithttp" => {
            let mode = clean(get_str("mode").as_ref());
            let x_padding_bytes = clean(get_str("x_padding_bytes").as_ref())
                .or_else(|| clean(get_str("xPaddingBytes").as_ref()));
            let mut extra: Map<String, Value> = Map::new();
            for key in XHTTP_EXTRA_KEYS {
                if let Some(v) = obj.get(key) {
                    extra.insert(key.to_owned(), v.clone());
                }
            }
            let headers = collect_headers(obj);
            Transport::Xhttp {
                path,
                host,
                mode,
                extra: (!extra.is_empty()).then_some(extra),
                x_padding_bytes,
                headers,
            }
        }
        "httpupgrade" => Transport::HttpUpgrade { path, host },
        "kcp" => Transport::Kcp { header_type },
        "quic" => Transport::Quic { header_type },
        other => {
            return Err(KvnError::Unsupported(format!(
                "vmess transport `net={other}`"
            )))
        }
    };

    let security = match get_str("tls")
        .map(|s| s.trim().to_ascii_lowercase())
        .as_deref()
    {
        None | Some("") | Some("none") => Security::None,
        Some("tls") => {
            let fingerprint = match clean(get_str("fp").as_ref()) {
                Some(fp) => Some(validate_fingerprint(&fp)?),
                None => None,
            };
            let alpn = get_str("alpn")
                .map(|raw| {
                    raw.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            Security::Tls(TlsConfig {
                sni: clean(get_str("sni").as_ref()),
                alpn,
                fingerprint,
                allow_insecure: matches!(
                    get_str("allowInsecure")
                        .map(|s| s.trim().to_ascii_lowercase())
                        .as_deref(),
                    Some("1") | Some("true")
                ),
                pinned_peer_cert_sha256: clean(get_str("pcs").as_ref()),
            })
        }
        Some(other) => return Err(KvnError::Unsupported(format!("vmess security `{other}`"))),
    };

    Ok(Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name: clean(get_str("ps").as_ref()).unwrap_or_default(),
        source: ProfileSource::UriImport,
        endpoint: Endpoint { address, port },
        protocol: ProtocolSpec::Vmess {
            uuid: Secret::new(uuid),
            alter_id,
            cipher,
        },
        transport,
        security,
    })
}

fn collect_headers(obj: &Map<String, Value>) -> Vec<Header> {
    match obj.get("headers") {
        Some(Value::Object(headers)) => headers
            .iter()
            .filter_map(|(k, v)| {
                v.as_str().map(|v| Header {
                    name: k.clone(),
                    value: v.to_owned(),
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(v: Value) -> String {
        use base64::Engine;
        let s = serde_json::to_string(&v).unwrap();
        format!(
            "vmess://{}",
            base64::engine::general_purpose::STANDARD.encode(s)
        )
    }

    #[test]
    fn parses_basic_vmess_ws_tls() {
        let uri = encode(serde_json::json!({
            "v": "2", "ps": "Relay-1", "add": "r.example.com", "port": "443",
            "id": "b831381d-6324-4d53-ad4f-8cda48b30811", "aid": "0", "scy": "auto",
            "net": "ws", "type": "none", "host": "cdn.example.com", "path": "/ray",
            "tls": "tls", "sni": "cdn.example.com", "fp": "chrome", "alpn": "h2,http/1.1"
        }));
        let p = parse(&uri).unwrap();
        assert_eq!(p.name, "Relay-1");
        assert_eq!(p.endpoint.address, "r.example.com");
        assert_eq!(p.endpoint.port, 443);
        match &p.transport {
            Transport::Ws { path, host, .. } => {
                assert_eq!(path.as_deref(), Some("/ray"));
                assert_eq!(host.as_deref(), Some("cdn.example.com"));
            }
            other => panic!("{other:?}"),
        }
        match &p.security {
            Security::Tls(t) => {
                assert_eq!(t.sni.as_deref(), Some("cdn.example.com"));
                assert_eq!(t.alpn, vec!["h2", "http/1.1"]);
                assert_eq!(t.fingerprint.as_deref(), Some("chrome"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_vmess_xhttp_with_flattened_extra() {
        let uri = encode(serde_json::json!({
            "add": "x.example.com", "port": 443, "id": "abc", "net": "xhttp",
            "path": "/sp", "host": "x.example.com", "mode": "packet-up",
            "x_padding_bytes": "100-1000",
            "sessionIDPlacement": "query", "scMaxEachPostBytes": "500000",
            "ps": "X"
        }));
        let p = parse(&uri).unwrap();
        match &p.transport {
            Transport::Xhttp {
                mode,
                x_padding_bytes,
                extra,
                ..
            } => {
                assert_eq!(mode.as_deref(), Some("packet-up"));
                assert_eq!(x_padding_bytes.as_deref(), Some("100-1000"));
                let extra = extra.as_ref().unwrap();
                assert_eq!(extra.get("sessionIDPlacement").unwrap(), "query");
                assert_eq!(extra.get("scMaxEachPostBytes").unwrap(), "500000");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn vmess_garbage_is_rejected() {
        assert!(parse("vmess://!!!not-base64!!!").is_err());
        use base64::Engine;
        let not_json = base64::engine::general_purpose::STANDARD.encode("plain text");
        assert!(parse(&format!("vmess://{not_json}")).is_err());
    }

    #[test]
    fn vmess_unknown_cipher_rejected() {
        let uri = encode(serde_json::json!({
            "add": "h", "port": 1, "id": "i", "scy": "aes-256-cfb"
        }));
        assert!(matches!(parse(&uri).unwrap_err(), KvnError::Unsupported(_)));
    }
}
