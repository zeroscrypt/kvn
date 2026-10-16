//! Trojan link parsing (shares transport/security mapping with VLESS).

use crate::error::{KvnError, Result};
use crate::profile::{Endpoint, Profile, ProfileSource, ProtocolSpec};
use crate::secret::Secret;

use super::common;
use super::{clean, RawLink};

pub(crate) fn parse(uri: &str) -> Result<Profile> {
    let link = RawLink::parse(uri)?;
    let q = &link.query;

    let password = link
        .userinfo
        .as_deref()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| KvnError::MissingField("password".into()))?;

    let flow = match clean(q.get("flow")) {
        None => None,
        Some(f) if f == "xtls-rprx-vision" || f == "xtls-rprx-vision-udp443" => Some(f),
        Some(other) => return Err(KvnError::Unsupported(format!("flow `{other}`"))),
    };

    let transport = common::transport_from_query(q)?;
    // Trojan defaults to TLS when the security parameter is absent.
    let security = common::security_from_query(q, true)?;

    Ok(Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name: link.fragment.clone().unwrap_or_default(),
        source: ProfileSource::UriImport,
        endpoint: Endpoint {
            address: link.host,
            port: link.port,
        },
        protocol: ProtocolSpec::Trojan {
            password: Secret::new(password),
            flow,
        },
        transport,
        security,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Security, Transport};

    #[test]
    fn parses_trojan_reality_vision() {
        let uri = "trojan://p%40ssw0rd%2Fwith%2Fslashes@t.example.com:443\
                   ?type=tcp&security=reality&pbk=K&sni=cover.example&sid=ab12&fp=firefox&flow=xtls-rprx-vision#Trojan-A";
        let p = parse(uri).unwrap();
        assert_eq!(p.name, "Trojan-A");
        match &p.protocol {
            ProtocolSpec::Trojan { password, flow } => {
                assert_eq!(password.expose(), "p@ssw0rd/with/slashes");
                assert_eq!(flow.as_deref(), Some("xtls-rprx-vision"));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(p.security, Security::Reality(_)));
        assert!(matches!(p.transport, Transport::Tcp { .. }));
    }

    #[test]
    fn trojan_tls_default_and_alpn() {
        let p = parse("trojan://pw@t.example.com:8443?sni=x.example&alpn=h2,http%2F1.1").unwrap();
        match &p.security {
            Security::Tls(t) => {
                assert_eq!(t.sni.as_deref(), Some("x.example"));
                assert_eq!(t.alpn, vec!["h2", "http/1.1"]);
                assert!(!t.allow_insecure);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn trojan_without_password_fails() {
        let e = parse("trojan://t.example.com:443").unwrap_err();
        assert!(matches!(e, KvnError::MissingField(ref f) if f == "password"));
    }
}
