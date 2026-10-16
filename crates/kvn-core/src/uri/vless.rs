//! VLESS link parsing, including VLESS + REALITY + XHTTP as emitted by
//! 3x-ui (`internal/sub/service.go` verified against v3.x).

use crate::error::{KvnError, Result};
use crate::profile::{Endpoint, Profile, ProfileSource, ProtocolSpec};
use crate::secret::Secret;

use super::{clean, RawLink};

pub(crate) fn parse(uri: &str) -> Result<Profile> {
    let link = RawLink::parse(uri)?;
    let q = &link.query;

    let uuid_raw = link
        .userinfo
        .as_deref()
        .filter(|u| !u.is_empty())
        .ok_or_else(|| KvnError::MissingField("uuid".into()))?;
    // VLESS ids are either UUIDs (32 hex digits, optionally dashed) or
    // custom strings shorter than 30 bytes (per Xray docs).
    let compact: String = uuid_raw.chars().filter(|c| *c != '-').collect();
    let is_uuid = compact.len() == 32 && compact.chars().all(|c| c.is_ascii_hexdigit());
    if !is_uuid && uuid_raw.len() >= 30 {
        return Err(KvnError::invalid(
            "uuid",
            "custom VLESS ids must be shorter than 30 bytes",
        ));
    }
    let uuid = if is_uuid {
        compact.to_ascii_lowercase()
    } else {
        uuid_raw.to_owned()
    };

    let encryption = clean(q.get("encryption")).unwrap_or_else(|| "none".to_owned());
    if encryption != "none" && !encryption.starts_with("mlkem768x25519plus") {
        return Err(KvnError::Unsupported(
            "VLESS encryption value (expected `none` or `mlkem768x25519plus...`)".to_owned(),
        ));
    }

    let flow = match clean(q.get("flow")) {
        None => None,
        Some(f) if f == "xtls-rprx-vision" || f == "xtls-rprx-vision-udp443" => Some(f),
        Some(other) => return Err(KvnError::Unsupported(format!("flow `{other}`"))),
    };

    let transport = super::common::transport_from_query(q)?;
    let security = super::common::security_from_query(q, false)?;

    Ok(Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name: link.fragment.clone().unwrap_or_default(),
        source: ProfileSource::UriImport,
        endpoint: Endpoint {
            address: link.host,
            port: link.port,
        },
        protocol: ProtocolSpec::Vless {
            uuid: Secret::new(uuid),
            flow,
            encryption,
        },
        transport,
        security,
    })
}

// Lowercase UUID-shaped ids; keep custom ids untouched.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Security, TcpHeader, Transport};

    #[test]
    fn parses_vless_reality_tcp_vision() {
        let uri = "vless://5783a3e7-e373-51cd-8642-c83782b807c5@example.com:443\
                   ?type=tcp&security=reality&pbk=BBBBpublic%20key&fp=chrome&sni=www.example.com\
                   &sid=0123456789abcdef&spx=%2F&flow=xtls-rprx-vision&encryption=none#Tokyo-01";
        let p = parse(uri).unwrap();
        assert_eq!(p.name, "Tokyo-01");
        assert_eq!(p.endpoint.address, "example.com");
        assert_eq!(p.endpoint.port, 443);
        match &p.protocol {
            ProtocolSpec::Vless {
                uuid,
                flow,
                encryption,
            } => {
                assert_eq!(uuid.expose(), "5783a3e7e37351cd8642c83782b807c5");
                assert_eq!(flow.as_deref(), Some("xtls-rprx-vision"));
                assert_eq!(encryption, "none");
            }
            other => panic!("wrong protocol: {other:?}"),
        }
        assert!(matches!(
            p.transport,
            Transport::Tcp {
                header_type: TcpHeader::None,
                ..
            }
        ));
        match &p.security {
            Security::Reality(r) => {
                assert_eq!(r.public_key, "BBBBpublic key");
                assert_eq!(r.server_name, "www.example.com");
                assert_eq!(r.short_id.as_deref(), Some("0123456789abcdef"));
                assert_eq!(r.spider_x.as_deref(), Some("/"));
                assert_eq!(r.fingerprint.as_deref(), Some("chrome"));
            }
            other => panic!("wrong security: {other:?}"),
        }
    }

    #[test]
    fn parses_vless_xhttp_reality_with_extra_blob() {
        // Shape emitted by 3x-ui: path/host/mode top-level + JSON `extra` +
        // flat `x_padding_bytes`.
        let uri = "vless://11111111-2222-3333-4444-555555555555@203.0.113.7:443\
                   ?type=xhttp&security=reality&pbk=PUBKEY&fp=chrome&sni=cdn.example.com&sid=a1b2\
                   &path=%2Fsplitstream&host=cdn.example.com&mode=auto\
                   &x_padding_bytes=100-1000\
                   &extra=%7B%22xPaddingBytes%22%3A%22100-1000%22%2C%22sessionIDPlacement%22%3A%22query%22%7D\
                   &support-x25519mlkem768=true#DE-Frankfurt";
        let p = parse(uri).unwrap();
        assert_eq!(p.name, "DE-Frankfurt");
        match &p.transport {
            Transport::Xhttp {
                path,
                host,
                mode,
                extra,
                x_padding_bytes,
                ..
            } => {
                assert_eq!(path.as_deref(), Some("/splitstream"));
                assert_eq!(host.as_deref(), Some("cdn.example.com"));
                assert_eq!(mode.as_deref(), Some("auto"));
                assert_eq!(x_padding_bytes.as_deref(), Some("100-1000"));
                let extra = extra.as_ref().expect("extra blob preserved");
                assert_eq!(extra.get("sessionIDPlacement").unwrap(), "query");
            }
            other => panic!("wrong transport: {other:?}"),
        }
        match &p.security {
            Security::Reality(r) => assert!(r.support_x25519mlkem768),
            other => panic!("wrong security: {other:?}"),
        }
    }

    #[test]
    fn vless_defaults_tcp_none() {
        let p = parse("vless://abc@host.example:8443").unwrap();
        assert!(matches!(p.transport, Transport::Tcp { .. }));
        assert!(matches!(p.security, Security::None));
        assert!(p.name.is_empty());
    }

    #[test]
    fn vless_reality_requires_pbk_and_sni() {
        let e = parse("vless://abc@host.example:443?security=reality&sni=x.com").unwrap_err();
        assert!(matches!(e, KvnError::MissingField(ref f) if f == "pbk"));
        let e = parse("vless://abc@host.example:443?security=reality&pbk=K").unwrap_err();
        assert!(matches!(e, KvnError::MissingField(ref f) if f == "sni"));
    }

    #[test]
    fn vless_rejects_unknown_security_and_transport() {
        assert!(parse("vless://abc@h.example:443?security=xtls").is_err());
        assert!(parse("vless://abc@h.example:443?type=carrierpigeon").is_err());
    }

    #[test]
    fn vless_rejects_bad_fingerprint() {
        let e = parse("vless://abc@h.example:443?security=reality&pbk=K&sni=s&fp=mybrowser")
            .unwrap_err();
        assert!(matches!(e, KvnError::InvalidValue { ref field, .. } if field == "fp"));
    }

    #[test]
    fn vless_encryption_mlkem_passthrough() {
        let uri = "vless://abc@h.example:443?encryption=mlkem768x25519plus.native.600s.100-111-1111.75-0-111.50-0-3333.ptjHQx";
        let p = parse(uri).unwrap();
        if let ProtocolSpec::Vless { encryption, .. } = &p.protocol {
            assert!(encryption.starts_with("mlkem768x25519plus"));
        } else {
            panic!();
        }
    }
}
