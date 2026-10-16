//! Xray client config generation.
//!
//! Turns a [`Profile`] into a complete, engine-validated client config:
//! local SOCKS/HTTP inbounds, the proxy outbound built from the profile,
//! optional traffic-stats API, routing with LAN/custom bypass, DNS, and
//! optional TLS fragment anti-DPI layer.
//!
//! Two output channels:
//! - [`build_client_config`] returns the config;
//! - [`BuildOutcome::notes`] lists anything that was dropped or that
//!   weakens security (e.g. `allowInsecure` imported from a link, an
//!   unknown XHTTP `extra` key the engine would reject). Notes are shown
//!   in the UI and in diagnostics — never silently swallowed.

use serde_json::{json, Map, Value};

use crate::error::{KvnError, Result};
use crate::profile::{Profile, ProtocolSpec, Security, TcpHeader, Transport};

pub const DEFAULT_SOCKS_PORT: u16 = 10808;
pub const DEFAULT_HTTP_PORT: u16 = 10809;

/// Private / link-local ranges bypassed in "bypass LAN" mode. Explicit CIDR
/// list instead of `geoip:private` so the config has no GeoIP file
/// dependency.
const PRIVATE_CIDRS: [&str; 9] = [
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "127.0.0.0/8",
    "169.254.0.0/16",
    "::1/128",
    "fc00::/7",
    "fe80::/10",
    "ff00::/8",
];

/// XHTTP settings the engine understands (Xray `SplitHTTPConfig` JSON
/// names). Anything else found in an imported `extra` blob is kept in the
/// profile but excluded from the config, with a note.
const KNOWN_XHTTP_KEYS: [&str; 28] = [
    "host",
    "path",
    "mode",
    "headers",
    "xPaddingBytes",
    "noGRPCHeader",
    "noSSEHeader",
    "scMaxEachPostBytes",
    "scMinPostsIntervalMs",
    "scMaxBufferedPosts",
    "scStreamUpServerSecs",
    "xmux",
    "downloadSettings",
    "xPaddingObfsMode",
    "xPaddingKey",
    "xPaddingHeader",
    "xPaddingPlacement",
    "xPaddingMethod",
    "uplinkHTTPMethod",
    "sessionIDPlacement",
    "sessionIDKey",
    "seqPlacement",
    "seqKey",
    "uplinkDataPlacement",
    "uplinkDataKey",
    "uplinkChunkSize",
    "serverMaxHeaderBytes",
    "sessionIDTable",
];

#[derive(Debug, Clone, PartialEq)]
pub struct FragmentOptions {
    /// `tlshello` (fragment the TLS ClientHello) is the useful default.
    pub packets: String,
    pub length: String,
    pub interval: String,
}

impl Default for FragmentOptions {
    fn default() -> Self {
        FragmentOptions {
            packets: "tlshello".into(),
            length: "100-200".into(),
            interval: "10-20".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub socks_port: u16,
    pub http_port: u16,
    /// Port for the local stats API (`dokodemo-door`). 0 disables it.
    pub api_port: u16,
    pub dns_remote: String,
    pub sniffing: bool,
    pub bypass_lan: bool,
    pub bypass_domains: Vec<String>,
    pub bypass_ips: Vec<String>,
    pub log_level: String,
    /// Path of the engine error log; `None` → stderr.
    pub error_log: Option<String>,
    pub fragment: Option<FragmentOptions>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions {
            socks_port: DEFAULT_SOCKS_PORT,
            http_port: DEFAULT_HTTP_PORT,
            api_port: 0,
            dns_remote: "1.1.1.1".into(),
            sniffing: true,
            bypass_lan: true,
            bypass_domains: Vec::new(),
            bypass_ips: Vec::new(),
            log_level: "warning".into(),
            error_log: None,
            fragment: None,
        }
    }
}

#[derive(Debug)]
pub struct BuildOutcome {
    pub config: Value,
    pub notes: Vec<String>,
}

pub fn build_client_config(profile: &Profile, opts: &BuildOptions) -> Result<BuildOutcome> {
    let mut notes: Vec<String> = Vec::new();

    validate_opts(opts)?;

    let mut inbounds: Vec<Value> = vec![socks_inbound(opts), http_inbound(opts)];
    let mut routing_rules: Vec<Value> = Vec::new();

    if opts.api_port > 0 {
        inbounds.push(api_inbound(opts.api_port));
        routing_rules.push(json!({
            "type": "field",
            "inboundTag": ["api"],
            "outboundTag": "api",
        }));
    }

    let mut outbounds: Vec<Value> = vec![proxy_outbound(profile, opts, &mut notes)?];
    if let Some(fragment) = &opts.fragment {
        outbounds.push(fragment_outbound(fragment));
    }
    outbounds.push(json!({ "tag": "direct", "protocol": "freedom", "settings": {} }));
    outbounds.push(json!({ "tag": "block", "protocol": "blackhole", "settings": {} }));

    if opts.bypass_lan {
        routing_rules.push(json!({
            "type": "field",
            "ip": PRIVATE_CIDRS,
            "outboundTag": "direct",
        }));
    }
    if !opts.bypass_domains.is_empty() {
        routing_rules.push(json!({
            "type": "field",
            "domain": opts.bypass_domains,
            "outboundTag": "direct",
        }));
    }
    if !opts.bypass_ips.is_empty() {
        routing_rules.push(json!({
            "type": "field",
            "ip": opts.bypass_ips,
            "outboundTag": "direct",
        }));
    }
    // Everything left goes through the proxy.
    routing_rules.push(json!({
        "type": "field",
        "network": "tcp,udp",
        "outboundTag": "proxy",
    }));

    let log = if let Some(path) = &opts.error_log {
        json!({ "loglevel": opts.log_level, "access": "none", "error": path, "dnsLog": false })
    } else {
        json!({ "loglevel": opts.log_level, "access": "none", "dnsLog": false })
    };

    let mut config = json!({
        "log": log,
        "dns": {
            "servers": [opts.dns_remote],
            "queryStrategy": "UseIP",
            "disableFallback": true,
        },
        "inbounds": inbounds,
        "outbounds": outbounds,
        "routing": {
            "domainStrategy": "IPIfNonMatch",
            "rules": routing_rules,
        },
        "policy": {
            "levels": {
                "8": { "connIdle": 300, "uplinkOnly": 1, "downlinkOnly": 1, "handshake": 4 },
            },
            "system": {
                "statsInboundUplink": true,
                "statsInboundDownlink": true,
                "statsOutboundUplink": true,
                "statsOutboundDownlink": true,
            },
        },
    });

    if opts.api_port > 0 {
        config["stats"] = json!({});
        config["api"] = json!({ "tag": "api", "services": ["StatsService"] });
    }

    Ok(BuildOutcome { config, notes })
}

fn validate_opts(opts: &BuildOptions) -> Result<()> {
    if opts.socks_port == 0 || opts.http_port == 0 {
        return Err(KvnError::invalid(
            "ports",
            "socks and http ports must be non-zero",
        ));
    }
    if opts.dns_remote.trim().is_empty() {
        return Err(KvnError::invalid("dns_remote", "must not be empty"));
    }
    for d in &opts.bypass_domains {
        if d.trim().is_empty() || d.chars().any(|c| c.is_whitespace() || c == '"') {
            return Err(KvnError::invalid(
                "bypass_domains",
                "entries must be non-empty domain patterns",
            ));
        }
    }
    for ip in &opts.bypass_ips {
        if ip.trim().is_empty() || ip.chars().any(|c| c.is_whitespace() || c == '"') {
            return Err(KvnError::invalid(
                "bypass_ips",
                "entries must be non-empty IP or CIDR values",
            ));
        }
    }
    Ok(())
}

fn sniffing_json(opts: &BuildOptions) -> Value {
    json!({
        "enabled": opts.sniffing,
        "destOverride": ["http", "tls", "quic"],
    })
}

fn socks_inbound(opts: &BuildOptions) -> Value {
    json!({
        "tag": "socks-in",
        "listen": "127.0.0.1",
        "port": opts.socks_port,
        "protocol": "socks",
        "settings": { "auth": "noauth", "udp": true, "userLevel": 8 },
        "sniffing": sniffing_json(opts),
    })
}

fn http_inbound(opts: &BuildOptions) -> Value {
    json!({
        "tag": "http-in",
        "listen": "127.0.0.1",
        "port": opts.http_port,
        "protocol": "http",
        "settings": { "userLevel": 8 },
        "sniffing": sniffing_json(opts),
    })
}

fn api_inbound(port: u16) -> Value {
    json!({
        "tag": "api",
        "listen": "127.0.0.1",
        "port": port,
        "protocol": "dokodemo-door",
        "settings": { "address": "127.0.0.1" },
    })
}

fn fragment_outbound(fragment: &FragmentOptions) -> Value {
    json!({
        "tag": "fragment",
        "protocol": "freedom",
        "settings": {
            "fragment": {
                "packets": fragment.packets,
                "length": fragment.length,
                "interval": fragment.interval,
            },
        },
    })
}

fn flow_is_applicable(transport: &Transport, security: &Security) -> bool {
    matches!(transport, Transport::Tcp { .. })
        && matches!(security, Security::Tls(_) | Security::Reality(_))
}

fn proxy_outbound(
    profile: &Profile,
    opts: &BuildOptions,
    notes: &mut Vec<String>,
) -> Result<Value> {
    let endpoint = &profile.endpoint;
    let (protocol, settings) = match &profile.protocol {
        ProtocolSpec::Vless {
            uuid,
            flow,
            encryption,
        } => {
            let mut user = json!({
                "id": uuid.expose(),
                "encryption": encryption,
                "level": 8,
            });
            if let Some(flow) = flow {
                if flow_is_applicable(&profile.transport, &profile.security) {
                    user["flow"] = json!(flow);
                } else {
                    notes.push(format!(
                        "flow `{flow}` omitted: XTLS Vision requires the TCP transport with TLS or REALITY"
                    ));
                }
            }
            (
                "vless",
                json!({
                    "vnext": [{
                        "address": endpoint.address,
                        "port": endpoint.port,
                        "users": [user],
                    }],
                }),
            )
        }
        ProtocolSpec::Vmess {
            uuid,
            alter_id,
            cipher,
        } => (
            "vmess",
            json!({
                "vnext": [{
                    "address": endpoint.address,
                    "port": endpoint.port,
                    "users": [{
                        "id": uuid.expose(),
                        "alterId": alter_id,
                        "security": cipher,
                        "level": 8,
                    }],
                }],
            }),
        ),
        ProtocolSpec::Trojan { password, flow } => {
            let mut server = json!({
                "address": endpoint.address,
                "port": endpoint.port,
                "password": password.expose(),
                "level": 8,
            });
            if let Some(flow) = flow {
                if flow_is_applicable(&profile.transport, &profile.security) {
                    server["flow"] = json!(flow);
                } else {
                    notes.push(format!(
                        "flow `{flow}` omitted: XTLS Vision requires the TCP transport with TLS or REALITY"
                    ));
                }
            }
            ("trojan", json!({ "servers": [server] }))
        }
        ProtocolSpec::Shadowsocks { method, password } => (
            "shadowsocks",
            json!({
                "servers": [{
                    "address": endpoint.address,
                    "port": endpoint.port,
                    "method": method,
                    "password": password.expose(),
                    "level": 8,
                }],
            }),
        ),
    };

    let stream = stream_settings(profile, opts, notes)?;
    Ok(json!({
        "tag": "proxy",
        "protocol": protocol,
        "settings": settings,
        "streamSettings": stream,
    }))
}

fn stream_settings(
    profile: &Profile,
    opts: &BuildOptions,
    notes: &mut Vec<String>,
) -> Result<Value> {
    let mut ss = Map::new();

    match &profile.transport {
        Transport::Tcp {
            header_type,
            path,
            host,
        } => {
            ss.insert("network".into(), json!("tcp"));
            if *header_type == TcpHeader::Http {
                let mut request = json!({
                    "headers": {},
                    "path": [path.clone().unwrap_or_else(|| "/".to_owned())],
                });
                if let Some(host) = host {
                    request["headers"]["Host"] = json!(host);
                }
                ss.insert(
                    "tcpSettings".into(),
                    json!({ "header": { "type": "http", "request": request } }),
                );
            }
        }
        Transport::Ws {
            path,
            host,
            headers,
        } => {
            ss.insert("network".into(), json!("ws"));
            let mut ws = Map::new();
            if let Some(path) = path {
                ws.insert("path".into(), json!(path));
            }
            if let Some(host) = host {
                ws.insert("host".into(), json!(host));
            }
            if !headers.is_empty() {
                let mut h = Map::new();
                for header in headers {
                    h.insert(header.name.clone(), json!(header.value));
                }
                ws.insert("headers".into(), Value::Object(h));
            }
            if !ws.is_empty() {
                ss.insert("wsSettings".into(), Value::Object(ws));
            }
        }
        Transport::Grpc {
            service_name,
            authority,
            multi_mode,
        } => {
            ss.insert("network".into(), json!("grpc"));
            let mut grpc = Map::new();
            if let Some(s) = service_name {
                grpc.insert("serviceName".into(), json!(s));
            }
            if let Some(a) = authority {
                grpc.insert("authority".into(), json!(a));
            }
            if *multi_mode {
                grpc.insert("multiMode".into(), json!(true));
            }
            if !grpc.is_empty() {
                ss.insert("grpcSettings".into(), Value::Object(grpc));
            }
        }
        Transport::Xhttp {
            path,
            host,
            mode,
            extra,
            x_padding_bytes,
            headers,
        } => {
            ss.insert("network".into(), json!("xhttp"));
            let mut xhttp = Map::new();
            if let Some(extra) = extra {
                for (key, value) in extra {
                    if KNOWN_XHTTP_KEYS.contains(&key.as_str()) {
                        xhttp.insert(key.clone(), value.clone());
                    } else {
                        notes.push(format!(
                            "unknown XHTTP extra key `{key}` kept in the profile but not sent to the engine"
                        ));
                    }
                }
            }
            // Explicit share-link parameters win over the extra blob
            // (mirrors Xray's own precedence for path/host/mode).
            if let Some(path) = path {
                xhttp.insert("path".into(), json!(path));
            }
            if let Some(host) = host {
                xhttp.insert("host".into(), json!(host));
            }
            if let Some(mode) = mode {
                xhttp.insert("mode".into(), json!(mode));
            }
            if let Some(xpb) = x_padding_bytes {
                xhttp
                    .entry("xPaddingBytes".to_owned())
                    .or_insert_with(|| json!(xpb));
            }
            if !headers.is_empty() {
                let mut h = Map::new();
                for header in headers {
                    h.insert(header.name.clone(), json!(header.value));
                }
                xhttp.insert("headers".into(), Value::Object(h));
            }
            ss.insert("xhttpSettings".into(), Value::Object(xhttp));
        }
        Transport::HttpUpgrade { path, host } => {
            ss.insert("network".into(), json!("httpupgrade"));
            let mut hu = Map::new();
            if let Some(path) = path {
                hu.insert("path".into(), json!(path));
            }
            if let Some(host) = host {
                hu.insert("host".into(), json!(host));
            }
            if !hu.is_empty() {
                ss.insert("httpupgradeSettings".into(), Value::Object(hu));
            }
        }
        Transport::Kcp { .. } | Transport::Quic { .. } => {
            return Err(KvnError::Unsupported(format!(
                "transport `{}` is not supported by the config generator yet",
                profile.transport.label()
            )));
        }
    }

    match &profile.security {
        Security::None => {
            ss.insert("security".into(), json!("none"));
        }
        Security::Tls(tls) => {
            ss.insert("security".into(), json!("tls"));
            let mut t = Map::new();
            if let Some(sni) = &tls.sni {
                t.insert("serverName".into(), json!(sni));
            }
            if let Some(fp) = &tls.fingerprint {
                t.insert("fingerprint".into(), json!(fp));
            }
            if !tls.alpn.is_empty() {
                t.insert("alpn".into(), json!(tls.alpn));
            }
            if tls.allow_insecure {
                t.insert("allowInsecure".into(), json!(true));
                notes.push(
                    "TLS certificate verification is disabled because the imported link sets allowInsecure"
                        .to_owned(),
                );
            }
            if let Some(pin) = &tls.pinned_peer_cert_sha256 {
                t.insert("pinnedPeerCertSha256".into(), json!(pin));
            }
            if !t.is_empty() {
                ss.insert("tlsSettings".into(), Value::Object(t));
            }
        }
        Security::Reality(reality) => {
            ss.insert("security".into(), json!("reality"));
            let mut r = Map::new();
            r.insert("serverName".into(), json!(reality.server_name));
            r.insert(
                "fingerprint".into(),
                json!(reality
                    .fingerprint
                    .clone()
                    .unwrap_or_else(|| "chrome".to_owned())),
            );
            r.insert("password".into(), json!(reality.public_key));
            if let Some(sid) = &reality.short_id {
                r.insert("shortId".into(), json!(sid));
            }
            if let Some(spx) = &reality.spider_x {
                r.insert("spiderX".into(), json!(spx));
            }
            if let Some(mldsa) = &reality.mldsa65_verify {
                r.insert("mldsa65Verify".into(), json!(mldsa));
            }
            r.insert("show".into(), json!(false));
            ss.insert("realitySettings".into(), Value::Object(r));
        }
    }

    if opts.fragment.is_some() {
        ss.insert("sockopt".into(), json!({ "dialerProxy": "fragment" }));
    }

    Ok(Value::Object(ss))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Endpoint, Header, RealityConfig, TcpHeader, TlsConfig, Transport};
    use crate::secret::Secret;

    fn vless(transport: Transport, security: Security, flow: Option<&str>) -> Profile {
        Profile::new(
            "t",
            Endpoint {
                address: "vpn.example.com".into(),
                port: 443,
            },
            ProtocolSpec::Vless {
                uuid: Secret::new("11111111-2222-3333-4444-555555555555"),
                flow: flow.map(str::to_owned),
                encryption: "none".into(),
            },
            transport,
            security,
        )
    }

    fn xhttp_reality() -> Profile {
        let mut extra = Map::new();
        extra.insert("sessionIDPlacement".into(), json!("query"));
        extra.insert("xPaddingBytes".into(), json!("100-1000"));
        extra.insert("evilUnknownKey".into(), json!(true));
        vless(
            Transport::Xhttp {
                path: Some("/sp".into()),
                host: Some("cdn.example.com".into()),
                mode: Some("auto".into()),
                extra: Some(extra),
                x_padding_bytes: Some("100-1000".into()),
                headers: vec![],
            },
            Security::Reality(RealityConfig {
                public_key: "PBK".into(),
                server_name: "cdn.example.com".into(),
                short_id: Some("ab".into()),
                fingerprint: Some("chrome".into()),
                spider_x: None,
                mldsa65_verify: None,
                support_x25519mlkem768: true,
            }),
            None,
        )
    }

    #[test]
    fn xhttp_reality_keeps_extra_and_drops_unknown_with_note() {
        let p = xhttp_reality();
        let outcome = build_client_config(&p, &BuildOptions::default()).unwrap();
        let proxy = &outcome.config["outbounds"][0];
        assert_eq!(proxy["protocol"], "vless");
        assert_eq!(proxy["streamSettings"]["network"], "xhttp");
        assert_eq!(proxy["streamSettings"]["security"], "reality");
        let rs = &proxy["streamSettings"]["realitySettings"];
        assert_eq!(rs["password"], "PBK");
        assert_eq!(rs["serverName"], "cdn.example.com");
        assert_eq!(rs["fingerprint"], "chrome");
        let xs = &proxy["streamSettings"]["xhttpSettings"];
        assert_eq!(xs["path"], "/sp");
        assert_eq!(xs["host"], "cdn.example.com");
        assert_eq!(xs["mode"], "auto");
        assert_eq!(xs["sessionIDPlacement"], "query");
        assert_eq!(xs["xPaddingBytes"], "100-1000");
        assert!(xs.get("evilUnknownKey").is_none());
        assert!(
            outcome.notes.iter().any(|n| n.contains("evilUnknownKey")),
            "unknown extra key must be reported: {:?}",
            outcome.notes
        );
    }

    #[test]
    fn vision_flow_only_for_tcp_tls_reality() {
        let reality = Security::Reality(RealityConfig {
            public_key: "K".into(),
            server_name: "s".into(),
            short_id: None,
            fingerprint: None,
            spider_x: None,
            mldsa65_verify: None,
            support_x25519mlkem768: false,
        });
        // TCP + REALITY → kept.
        let p = vless(
            Transport::Tcp {
                header_type: TcpHeader::None,
                path: None,
                host: None,
            },
            reality.clone(),
            Some("xtls-rprx-vision"),
        );
        let out = build_client_config(&p, &BuildOptions::default()).unwrap();
        assert_eq!(
            out.config["outbounds"][0]["settings"]["vnext"][0]["users"][0]["flow"],
            "xtls-rprx-vision"
        );

        // XHTTP + REALITY → omitted with a note.
        let mut p2 = xhttp_reality();
        if let ProtocolSpec::Vless { flow, .. } = &mut p2.protocol {
            *flow = Some("xtls-rprx-vision".into());
        }
        let out2 = build_client_config(&p2, &BuildOptions::default()).unwrap();
        assert!(
            out2.config["outbounds"][0]["settings"]["vnext"][0]["users"][0]
                .get("flow")
                .is_none()
        );
        assert!(out2.notes.iter().any(|n| n.contains("omitted")));
    }

    #[test]
    fn tcp_reality_sets_network_and_default_fingerprint() {
        let p = vless(
            Transport::Tcp {
                header_type: TcpHeader::None,
                path: None,
                host: None,
            },
            Security::Reality(RealityConfig {
                public_key: "K".into(),
                server_name: "s.example".into(),
                short_id: None,
                fingerprint: None, // defaulted to chrome
                spider_x: None,
                mldsa65_verify: None,
                support_x25519mlkem768: false,
            }),
            None,
        );
        let out = build_client_config(&p, &BuildOptions::default()).unwrap();
        let settings = &out.config["outbounds"][0]["streamSettings"];
        assert_eq!(settings["network"], "tcp");
        assert_eq!(settings["realitySettings"]["fingerprint"], "chrome");
    }

    #[test]
    fn tls_allow_insecure_is_flagged() {
        let p = vless(
            Transport::Ws {
                path: Some("/ws".into()),
                host: None,
                headers: vec![Header {
                    name: "X-Test".into(),
                    value: "1".into(),
                }],
            },
            Security::Tls(TlsConfig {
                sni: Some("s.example".into()),
                alpn: vec!["h2".into()],
                fingerprint: None,
                allow_insecure: true,
                pinned_peer_cert_sha256: None,
            }),
            None,
        );
        let out = build_client_config(&p, &BuildOptions::default()).unwrap();
        let tls = &out.config["outbounds"][0]["streamSettings"]["tlsSettings"];
        assert_eq!(tls["allowInsecure"], true);
        assert_eq!(tls["alpn"][0], "h2");
        assert_eq!(
            out.config["outbounds"][0]["streamSettings"]["wsSettings"]["headers"]["X-Test"],
            "1"
        );
        assert!(out.notes.iter().any(|n| n.contains("allowInsecure")));
    }

    #[test]
    fn local_inbounds_and_routing_are_complete() {
        let opts = BuildOptions {
            api_port: 10085,
            ..Default::default()
        };
        let out = build_client_config(&xhttp_reality(), &opts).unwrap();
        let inbounds = out.config["inbounds"].as_array().unwrap();
        assert_eq!(inbounds.len(), 3); // socks, http, api
        assert_eq!(inbounds[0]["tag"], "socks-in");
        assert_eq!(inbounds[1]["tag"], "http-in");
        assert_eq!(inbounds[2]["protocol"], "dokodemo-door");

        let rules = out.config["routing"]["rules"].as_array().unwrap();
        assert_eq!(rules[0]["inboundTag"][0], "api");
        assert_eq!(rules[0]["outboundTag"], "api");
        let last = rules.last().unwrap();
        assert_eq!(last["outboundTag"], "proxy");
        assert!(rules
            .iter()
            .any(|r| r["outboundTag"] == "direct" && r["ip"][0] == "10.0.0.0/8"));

        assert!(out.config["stats"].is_object());
        assert_eq!(out.config["api"]["services"][0], "StatsService");
    }

    #[test]
    fn bypass_lists_become_direct_rules() {
        let opts = BuildOptions {
            bypass_domains: vec!["domain:example.org".into(), "geosite:private".into()],
            bypass_ips: vec!["203.0.113.0/24".into()],
            ..Default::default()
        };
        let out = build_client_config(&xhttp_reality(), &opts).unwrap();
        let rules = out.config["routing"]["rules"].as_array().unwrap();
        assert!(rules.iter().any(|r| r["domain"][0] == "domain:example.org"));
        assert!(rules.iter().any(|r| r["ip"][0] == "203.0.113.0/24"));
    }

    #[test]
    fn fragment_adds_outbound_and_dialer_proxy() {
        let opts = BuildOptions {
            fragment: Some(FragmentOptions::default()),
            ..Default::default()
        };
        let out = build_client_config(&xhttp_reality(), &opts).unwrap();
        let outbounds = out.config["outbounds"].as_array().unwrap();
        assert!(outbounds
            .iter()
            .any(|o| o["tag"] == "fragment" && o["settings"]["fragment"]["packets"] == "tlshello"));
        assert_eq!(
            outbounds[0]["streamSettings"]["sockopt"]["dialerProxy"],
            "fragment"
        );
    }

    #[test]
    fn kcp_and_quic_are_explicitly_unsupported() {
        for transport in [
            Transport::Kcp { header_type: None },
            Transport::Quic { header_type: None },
        ] {
            let p = vless(transport, Security::None, None);
            let err = build_client_config(&p, &BuildOptions::default()).unwrap_err();
            assert!(matches!(err, KvnError::Unsupported(_)));
        }
    }

    #[test]
    fn trojan_and_ss_credentials_reach_settings() {
        let t = Profile::new(
            "t",
            Endpoint {
                address: "h".into(),
                port: 8443,
            },
            ProtocolSpec::Trojan {
                password: Secret::new("pw"),
                flow: None,
            },
            Transport::Tcp {
                header_type: TcpHeader::None,
                path: None,
                host: None,
            },
            Security::Tls(TlsConfig::default()),
        );
        let out = build_client_config(&t, &BuildOptions::default()).unwrap();
        assert_eq!(out.config["outbounds"][0]["protocol"], "trojan");
        assert_eq!(
            out.config["outbounds"][0]["settings"]["servers"][0]["password"],
            "pw"
        );

        let s = Profile::new(
            "s",
            Endpoint {
                address: "h".into(),
                port: 8388,
            },
            ProtocolSpec::Shadowsocks {
                method: "2022-blake3-aes-256-gcm".into(),
                password: Secret::new("key"),
            },
            Transport::Tcp {
                header_type: TcpHeader::None,
                path: None,
                host: None,
            },
            Security::None,
        );
        let out = build_client_config(&s, &BuildOptions::default()).unwrap();
        assert_eq!(out.config["outbounds"][0]["protocol"], "shadowsocks");
        assert_eq!(
            out.config["outbounds"][0]["settings"]["servers"][0]["method"],
            "2022-blake3-aes-256-gcm"
        );
    }

    #[test]
    fn bypass_list_validation() {
        let opts = BuildOptions {
            bypass_domains: vec!["bad domain with spaces".into()],
            ..Default::default()
        };
        assert!(build_client_config(&xhttp_reality(), &opts).is_err());
    }
}
