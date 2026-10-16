//! Readiness and traffic health checks.
//!
//! "Process started" is never treated as "connection works":
//! - [`wait_for_port`] waits until the local proxy port accepts TCP
//!   connections (engine listeners are up);
//! - [`check_through_socks`] performs a full end-to-end check *through the
//!   tunnel*: SOCKS5 CONNECT (with remote DNS resolution) to a
//!   `generate_204`-style endpoint and an HTTP request, measuring latency.
//!
//! The SOCKS5 client below implements the standard handshake only
//! (RFC 1928): no cryptography is involved.

use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::EngineError;

/// A plain-HTTP `generate_204`-style probe target. Plain HTTP is deliberate:
/// the check must not depend on TLS interception quirks, and the request
/// goes *through the tunnel*, so it proves real end-to-end connectivity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthTarget {
    pub host: String,
    pub http_path: String,
    pub port: u16,
}

impl HealthTarget {
    pub fn new(host: impl Into<String>, http_path: impl Into<String>) -> Self {
        HealthTarget {
            host: host.into(),
            http_path: http_path.into(),
            port: 80,
        }
    }
}

/// Ordered default targets; the first reachable one wins.
pub fn default_targets() -> Vec<HealthTarget> {
    vec![
        HealthTarget::new("cp.cloudflare.com", "/generate_204"),
        HealthTarget::new("www.gstatic.com", "/generate_204"),
        HealthTarget::new("detectportal.firefox.com", "/success.txt"),
    ]
}

/// Wait until `127.0.0.1:port` accepts connections, or fail after `timeout`.
pub async fn wait_for_port(port: u16, timeout: Duration) -> Result<(), EngineError> {
    let deadline = Instant::now() + timeout;
    loop {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(EngineError::NotReady(format!(
                "local proxy port {port} did not open within {:?}",
                timeout
            )));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// End-to-end check through the tunnel: returns latency in milliseconds,
/// measured from starting the connection until the response head arrives.
/// Any HTTP status below 400 counts as reachable.
pub async fn check_through_socks(
    socks_port: u16,
    target: &HealthTarget,
    timeout: Duration,
) -> Result<u32, EngineError> {
    let result = tokio::time::timeout(timeout, check_inner(socks_port, target)).await;
    match result {
        Ok(Ok(latency_ms)) => Ok(latency_ms),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(EngineError::Health(format!(
            "no response through the tunnel within {:?}",
            timeout
        ))),
    }
}

/// Try each target in order; returns the first success with its latency.
pub async fn check_through_socks_any(
    socks_port: u16,
    targets: &[HealthTarget],
    timeout_per_target: Duration,
) -> Result<u32, EngineError> {
    let mut last_err = None;
    for target in targets {
        match check_through_socks(socks_port, target, timeout_per_target).await {
            Ok(ms) => return Ok(ms),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| EngineError::Health("no health targets configured".into())))
}

async fn check_inner(socks_port: u16, target: &HealthTarget) -> Result<u32, EngineError> {
    let started = Instant::now();
    let mut stream = TcpStream::connect(("127.0.0.1", socks_port))
        .await
        .map_err(|e| EngineError::Health(format!("cannot reach the local proxy port: {e}")))?;
    socks5_connect(&mut stream, &target.host, target.port).await?;

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: kvn-health\r\nConnection: close\r\n\r\n",
        target.http_path, target.host
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| EngineError::Health(format!("failed to send the probe request: {e}")))?;

    // Read until the end of the response head; we only need the status line.
    let mut head = Vec::with_capacity(512);
    let mut buf = [0u8; 512];
    loop {
        let n = stream
            .read(&mut buf)
            .await
            .map_err(|e| EngineError::Health(format!("failed to read the probe response: {e}")))?;
        if n == 0 {
            break;
        }
        head.extend_from_slice(&buf[..n]);
        if head.windows(4).any(|w| w == b"\r\n\r\n") || head.len() > 8192 {
            break;
        }
    }
    let latency_ms = started.elapsed().as_millis() as u32;

    let text = String::from_utf8_lossy(&head);
    let status_line = text.lines().next().unwrap_or_default();
    let status: Option<u16> = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok());
    let verdict = match status {
        Some(code) if code < 400 => Ok(()),
        Some(code) => Err(EngineError::Health(format!(
            "probe target answered with HTTP {code}"
        ))),
        None => Err(EngineError::Health(
            "probe target did not answer with HTTP".into(),
        )),
    };

    // Drain the rest of the response (bounded in size and time) so the
    // tunnel-side counters settle and the connection closes cleanly. This
    // part is excluded from the reported latency.
    let _ = tokio::time::timeout(Duration::from_millis(1500), async {
        let mut drained = 0usize;
        while drained < 64 * 1024 {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => drained += n,
            }
        }
    })
    .await;

    verdict.map(|_| latency_ms)
}

/// SOCKS5 CONNECT with ATYP=domain, so name resolution happens on the
/// remote side (this is what makes the check meaningful for leaks).
async fn socks5_connect(stream: &mut TcpStream, host: &str, port: u16) -> Result<(), EngineError> {
    // Greeting: version 5, one method, no-auth.
    stream
        .write_all(&[0x05, 0x01, 0x00])
        .await
        .map_err(|e| EngineError::Health(format!("socks handshake write failed: {e}")))?;
    let mut method_reply = [0u8; 2];
    stream
        .read_exact(&mut method_reply)
        .await
        .map_err(|e| EngineError::Health(format!("socks handshake read failed: {e}")))?;
    if method_reply != [0x05, 0x00] {
        return Err(EngineError::Health(
            "local proxy refused the no-auth SOCKS5 handshake".into(),
        ));
    }

    let request = encode_socks5_connect(host, port)?;
    stream
        .write_all(&request)
        .await
        .map_err(|e| EngineError::Health(format!("socks connect write failed: {e}")))?;

    let mut head = [0u8; 4];
    stream
        .read_exact(&mut head)
        .await
        .map_err(|e| EngineError::Health(format!("socks connect read failed: {e}")))?;
    if head[0] != 0x05 {
        return Err(EngineError::Health(
            "unexpected SOCKS version in reply".into(),
        ));
    }
    if head[1] != 0x00 {
        return Err(EngineError::Health(format!(
            "SOCKS5 CONNECT rejected with code 0x{:02x}",
            head[1]
        )));
    }
    match head[3] {
        0x01 => {
            let mut rest = [0u8; 6];
            stream
                .read_exact(&mut rest)
                .await
                .map_err(EngineError::Io)?;
        }
        0x04 => {
            let mut rest = [0u8; 18];
            stream
                .read_exact(&mut rest)
                .await
                .map_err(EngineError::Io)?;
        }
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await.map_err(EngineError::Io)?;
            let mut rest = vec![0u8; len[0] as usize + 2];
            stream
                .read_exact(&mut rest)
                .await
                .map_err(EngineError::Io)?;
        }
        other => {
            return Err(EngineError::Health(format!(
                "unknown SOCKS5 address type 0x{other:02x}"
            )));
        }
    }
    Ok(())
}

/// Pure encoder, unit-tested separately.
fn encode_socks5_connect(host: &str, port: u16) -> Result<Vec<u8>, EngineError> {
    let host_bytes = host.as_bytes();
    if host_bytes.is_empty() || host_bytes.len() > 255 {
        return Err(EngineError::Health("invalid probe target host".into()));
    }
    let mut request = Vec::with_capacity(7 + host_bytes.len());
    request.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, host_bytes.len() as u8]);
    request.extend_from_slice(host_bytes);
    request.extend_from_slice(&port.to_be_bytes());
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socks5_connect_request_layout() {
        let request = encode_socks5_connect("example.com", 443).unwrap();
        assert_eq!(&request[..4], &[0x05, 0x01, 0x00, 0x03]);
        assert_eq!(request[4] as usize, "example.com".len());
        assert_eq!(&request[5..5 + 11], b"example.com");
        assert_eq!(&request[16..18], &443u16.to_be_bytes());
    }

    #[test]
    fn socks5_connect_rejects_bad_hosts() {
        assert!(encode_socks5_connect("", 80).is_err());
        assert!(encode_socks5_connect(&"a".repeat(256), 80).is_err());
    }

    #[test]
    fn default_targets_are_https_style_204_endpoints_over_http() {
        let targets = default_targets();
        assert!(!targets.is_empty());
        assert!(targets
            .iter()
            .all(|t| t.port == 80 && !t.http_path.is_empty()));
    }
}
