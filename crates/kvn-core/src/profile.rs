//! The typed profile model.
//!
//! A [`Profile`] is a fully normalized description of one server: where to
//! connect (`endpoint`), how to authenticate (`protocol`), how to carry the
//! bytes (`transport`) and what protects the connection (`security`).
//!
//! The model intentionally stores *everything* the engine can consume —
//! unknown import parameters that we cannot map are rejected during parsing
//! with a clear error instead of being silently dropped, and known-but-
//! extra parameters (e.g. XHTTP `extra` blobs) are preserved verbatim.

use crate::secret::Secret;
use serde::{Deserialize, Serialize};

pub type ProfileId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Vless,
    Vmess,
    Trojan,
    Shadowsocks,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Vless => "vless",
            Protocol::Vmess => "vmess",
            Protocol::Trojan => "trojan",
            Protocol::Shadowsocks => "shadowsocks",
        }
    }
}

/// Protocol-specific credentials and knobs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProtocolSpec {
    Vless {
        uuid: Secret,
        /// `xtls-rprx-vision` when the server requires XTLS Vision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flow: Option<String>,
        /// `none` or a VLESS Encryption string (mlkem768x25519plus...).
        #[serde(default = "default_vless_encryption")]
        encryption: String,
    },
    Vmess {
        uuid: Secret,
        #[serde(default)]
        alter_id: u16,
        /// VMess cipher: auto / aes-128-gcm / chacha20-poly1305 / none / zero.
        #[serde(default = "default_vmess_cipher")]
        cipher: String,
    },
    Trojan {
        password: Secret,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flow: Option<String>,
    },
    Shadowsocks {
        method: String,
        password: Secret,
    },
}

fn default_vless_encryption() -> String {
    "none".to_owned()
}

fn default_vmess_cipher() -> String {
    "auto".to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub address: String,
    pub port: u16,
}

impl Endpoint {
    pub fn label(&self) -> String {
        if self.address.contains(':') {
            // IPv6 literal
            format!("[{}]:{}", self.address, self.port)
        } else {
            format!("{}:{}", self.address, self.port)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TcpHeader {
    #[default]
    None,
    Http,
}

/// Transport (how the stream is carried). Tagged by `network`, matching the
/// names used in share links (`type=` parameter) and Xray configs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "network", rename_all = "lowercase")]
pub enum Transport {
    Tcp {
        #[serde(default)]
        header_type: TcpHeader,
        /// Path/host for TCP-HTTP camouflage (`headerType=http`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host: Option<String>,
    },
    Ws {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        headers: Vec<Header>,
    },
    Grpc {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        service_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        authority: Option<String>,
        #[serde(default)]
        multi_mode: bool,
    },
    Xhttp {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host: Option<String>,
        /// auto / packet-up / stream-up / stream-one
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<String>,
        /// Verbatim `extra` blob preserved from the share link.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extra: Option<serde_json::Map<String, serde_json::Value>>,
        /// Flat `x_padding_bytes` param (sing-box family).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x_padding_bytes: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        headers: Vec<Header>,
    },
    HttpUpgrade {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host: Option<String>,
    },
    /// Parsed but not yet buildable into an engine config (see docs/COMPATIBILITY.md).
    Kcp {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        header_type: Option<String>,
    },
    /// Parsed but not yet buildable into an engine config.
    Quic {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        header_type: Option<String>,
    },
}

impl Transport {
    pub fn label(&self) -> &'static str {
        match self {
            Transport::Tcp { .. } => "tcp",
            Transport::Ws { .. } => "ws",
            Transport::Grpc { .. } => "grpc",
            Transport::Xhttp { .. } => "xhttp",
            Transport::HttpUpgrade { .. } => "httpupgrade",
            Transport::Kcp { .. } => "kcp",
            Transport::Quic { .. } => "quic",
        }
    }
}

/// Connection security layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Security {
    None,
    Tls(TlsConfig),
    Reality(RealityConfig),
}

impl Security {
    pub fn label(&self) -> &'static str {
        match self {
            Security::None => "none",
            Security::Tls(_) => "tls",
            Security::Reality(_) => "reality",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TlsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sni: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alpn: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    /// True only when the imported link explicitly asked for it. Recorded so
    /// the UI and diagnostics can warn; never set by our own defaults.
    #[serde(default)]
    pub allow_insecure: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_peer_cert_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealityConfig {
    /// Server public key (`pbk`).
    pub public_key: String,
    /// `serverName` (SNI). Required by the engine.
    pub server_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spider_x: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mldsa65_verify: Option<String>,
    #[serde(default)]
    pub support_x25519mlkem768: bool,
}

/// Where the profile came from. Subscription URLs are *not* stored here —
/// they live in a separate entity so tokens do not get copied around.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileSource {
    #[default]
    Manual,
    UriImport,
    Subscription {
        subscription_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    #[serde(default)]
    pub source: ProfileSource,
    pub endpoint: Endpoint,
    pub protocol: ProtocolSpec,
    pub transport: Transport,
    pub security: Security,
}

impl Profile {
    pub fn new(
        name: impl Into<String>,
        endpoint: Endpoint,
        protocol: ProtocolSpec,
        transport: Transport,
        security: Security,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            source: ProfileSource::Manual,
            endpoint,
            protocol,
            transport,
            security,
        }
    }

    pub fn protocol(&self) -> Protocol {
        match &self.protocol {
            ProtocolSpec::Vless { .. } => Protocol::Vless,
            ProtocolSpec::Vmess { .. } => Protocol::Vmess,
            ProtocolSpec::Trojan { .. } => Protocol::Trojan,
            ProtocolSpec::Shadowsocks { .. } => Protocol::Shadowsocks,
        }
    }

    /// Short human label: "vless · reality · xhttp".
    pub fn summary(&self) -> String {
        format!(
            "{} · {} · {}",
            self.protocol().as_str(),
            self.security.label(),
            self.transport.label()
        )
    }

    /// True when `other` points at the same server with the same
    /// credentials — used to deduplicate imports. Display name is ignored.
    pub fn same_server_as(&self, other: &Profile) -> bool {
        self.endpoint == other.endpoint
            && self.protocol == other.protocol
            && self.transport == other.transport
            && self.security == other.security
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Profile {
        Profile::new(
            "Test",
            Endpoint {
                address: "vpn.example.com".into(),
                port: 443,
            },
            ProtocolSpec::Vless {
                uuid: Secret::new("u-1"),
                flow: None,
                encryption: "none".into(),
            },
            Transport::Xhttp {
                path: Some("/p".into()),
                host: None,
                mode: Some("auto".into()),
                extra: None,
                x_padding_bytes: None,
                headers: vec![],
            },
            Security::Reality(RealityConfig {
                public_key: "PUB".into(),
                server_name: "example.com".into(),
                short_id: None,
                fingerprint: None,
                spider_x: None,
                mldsa65_verify: None,
                support_x25519mlkem768: false,
            }),
        )
    }

    #[test]
    fn summary_is_readable() {
        assert_eq!(sample().summary(), "vless · reality · xhttp");
    }

    #[test]
    fn endpoint_label_handles_ipv6() {
        let e = Endpoint {
            address: "2001:db8::1".into(),
            port: 443,
        };
        assert_eq!(e.label(), "[2001:db8::1]:443");
    }

    #[test]
    fn profile_debug_never_contains_credentials() {
        let p = sample();
        let dbg = format!("{p:?}");
        assert!(!dbg.contains("u-1"));
        assert!(dbg.contains("Secret(***)") || dbg.contains("***"));
    }

    #[test]
    fn same_server_ignores_name_and_id() {
        let a = sample();
        let mut b = sample();
        b.name = "Renamed".into();
        b.id = "other-id".into();
        assert!(a.same_server_as(&b));
        b.endpoint.port = 8443;
        assert!(!a.same_server_as(&b));
    }

    #[test]
    fn profile_json_roundtrip_preserves_xhttp_extra() {
        let mut p = sample();
        let mut extra = serde_json::Map::new();
        extra.insert("xPaddingBytes".into(), serde_json::json!("100-1000"));
        extra.insert("sessionIDPlacement".into(), serde_json::json!("query"));
        if let Transport::Xhttp { extra: e, .. } = &mut p.transport {
            *e = Some(extra.clone());
        }
        let json = serde_json::to_string(&p).unwrap();
        let back: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        if let Transport::Xhttp {
            extra: Some(got), ..
        } = &back.transport
        {
            assert_eq!(got.get("sessionIDPlacement").unwrap(), "query");
        } else {
            panic!("extra lost in roundtrip");
        }
    }
}
