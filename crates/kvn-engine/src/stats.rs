//! Traffic counters via the engine's own stats API.
//!
//! The engine is started with a local `dokodemo-door` API inbound and
//! `StatsService` enabled; we shell out to `xray api statsquery` (the same
//! binary we already manage) instead of pulling in a gRPC stack. This keeps
//! the app dependency-light and reuses the engine's own tooling.

use std::path::Path;
use std::time::Duration;

use crate::error::EngineError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrafficSnapshot {
    pub uplink_bytes: u64,
    pub downlink_bytes: u64,
}

impl TrafficSnapshot {
    pub fn total_bytes(self) -> u64 {
        self.uplink_bytes.saturating_add(self.downlink_bytes)
    }
}

/// Query traffic counters.
///
/// Xray accounts traffic per inbound and per outbound; outbound uplink for
/// proxied (VLESS/VMess...) connections is not reliably reported, while
/// inbound counters always are. So: sum the local inbounds (socks + http,
/// excluding our own `api` traffic), falling back to the `proxy` outbound
/// counters when inbound stats are empty.
pub async fn query_stats(
    binary: &Path,
    api_port: u16,
    timeout: Duration,
) -> Result<TrafficSnapshot, EngineError> {
    let mut cmd = tokio::process::Command::new(binary);
    cmd.arg("api")
        .arg("statsquery")
        .arg(format!("--server=127.0.0.1:{api_port}"))
        .arg("-pattern")
        .arg(">>>traffic>>>")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| EngineError::Stats("stats query timed out".into()))?
        .map_err(|e| EngineError::Stats(format!("could not run the engine: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let text = stderr.trim();
        return Err(EngineError::Stats(if text.is_empty() {
            "the engine stats API did not answer".to_owned()
        } else {
            format!(
                "the engine stats API did not answer: {}",
                truncate(text, 500)
            )
        }));
    }
    parse_stats_json(&String::from_utf8_lossy(&output.stdout))
}

/// Parse `xray api statsquery` JSON. Unknown entries are ignored; a query
/// with no counters yet legitimately returns an empty result.
pub fn parse_stats_json(text: &str) -> Result<TrafficSnapshot, EngineError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(TrafficSnapshot::default());
    }
    let value: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|_| EngineError::Stats("stats API returned invalid JSON".into()))?;

    let mut inbound_up = 0u64;
    let mut inbound_down = 0u64;
    let mut outbound_up = 0u64;
    let mut outbound_down = 0u64;

    if let Some(entries) = value.get("stat").and_then(|s| s.as_array()) {
        for entry in entries {
            let name = entry
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or_default();
            let count: u64 = entry
                .get("value")
                .and_then(|v| {
                    // The engine emits numbers when a counter is non-zero and
                    // omits the field entirely when it is zero; some builds
                    // quote the number as a string. Accept both.
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                })
                .unwrap_or(0);
            let is_user_inbound =
                name.starts_with("inbound>>>") && !name.starts_with("inbound>>>api>>>");
            let is_proxy_outbound = name.starts_with("outbound>>>proxy>>>");
            if is_user_inbound && name.ends_with(">>>uplink") {
                inbound_up = inbound_up.saturating_add(count);
            } else if is_user_inbound && name.ends_with(">>>downlink") {
                inbound_down = inbound_down.saturating_add(count);
            } else if is_proxy_outbound && name.ends_with(">>>uplink") {
                outbound_up = outbound_up.saturating_add(count);
            } else if is_proxy_outbound && name.ends_with(">>>downlink") {
                outbound_down = outbound_down.saturating_add(count);
            }
        }
    }

    Ok(TrafficSnapshot {
        uplink_bytes: if inbound_up > 0 {
            inbound_up
        } else {
            outbound_up
        },
        downlink_bytes: if inbound_down > 0 {
            inbound_down
        } else {
            outbound_down
        },
    })
}

fn truncate(text: &str, max: usize) -> String {
    crate::truncate_chars(text, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_user_inbounds_and_excludes_api() {
        // Values arrive as JSON numbers from the live engine.
        let json = r#"{
            "stat": [
                {"name": "inbound>>>socks-in>>>traffic>>>uplink", "value": 92},
                {"name": "inbound>>>socks-in>>>traffic>>>downlink", "value": 839},
                {"name": "inbound>>>http-in>>>traffic>>>uplink", "value": 8},
                {"name": "inbound>>>api>>>traffic>>>uplink", "value": 188},
                {"name": "inbound>>>api>>>traffic>>>downlink", "value": 54},
                {"name": "outbound>>>proxy>>>traffic>>>downlink", "value": 829}
            ]
        }"#;
        let snapshot = parse_stats_json(json).unwrap();
        assert_eq!(snapshot.uplink_bytes, 100);
        assert_eq!(snapshot.downlink_bytes, 839);
        assert_eq!(snapshot.total_bytes(), 939);
    }

    #[test]
    fn accepts_quoted_values_too() {
        let json = r#"{
            "stat": [
                {"name": "inbound>>>socks-in>>>traffic>>>uplink", "value": "92"},
                {"name": "inbound>>>socks-in>>>traffic>>>downlink", "value": "58"}
            ]
        }"#;
        let snapshot = parse_stats_json(json).unwrap();
        assert_eq!(snapshot.total_bytes(), 150);
    }

    #[test]
    fn falls_back_to_proxy_outbound_counters() {
        let json = r#"{
            "stat": [
                {"name": "outbound>>>proxy>>>traffic>>>uplink", "value": 11},
                {"name": "outbound>>>proxy>>>traffic>>>downlink", "value": 22}
            ]
        }"#;
        let snapshot = parse_stats_json(json).unwrap();
        assert_eq!(snapshot.uplink_bytes, 11);
        assert_eq!(snapshot.downlink_bytes, 22);
    }

    #[test]
    fn empty_result_is_zero_not_error() {
        assert_eq!(parse_stats_json("").unwrap(), TrafficSnapshot::default());
        assert_eq!(parse_stats_json("{}").unwrap(), TrafficSnapshot::default());
        assert_eq!(
            parse_stats_json(r#"{"stat": []}"#).unwrap(),
            TrafficSnapshot::default()
        );
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_stats_json("not json").is_err());
    }
}
