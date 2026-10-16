//! Engine identification: run `<binary> version` and parse the output.
//!
//! The engine is only trusted after this probe succeeds, so a random file
//! dropped at the configured path cannot be executed as a tunnel.

use std::path::Path;
use std::time::Duration;

use crate::error::EngineError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineVersion {
    pub version: String,
    /// First line of the probe output, kept for diagnostics.
    pub raw: String,
}

/// Parse the first line of `xray version` output, e.g.
/// `Xray 26.3.27 (Xray, Penetrates Everything.) d2758a0 (go1.26.1 ...)`.
pub fn parse_version_output(raw: &str) -> Option<EngineVersion> {
    let line = raw.lines().next()?.trim();
    let mut parts = line.split_whitespace();
    let name = parts.next()?;
    if !name.eq_ignore_ascii_case("xray") {
        return None;
    }
    let version = parts.next()?;
    if !version.chars().next()?.is_ascii_digit() {
        return None;
    }
    Some(EngineVersion {
        version: version.to_owned(),
        raw: line.to_owned(),
    })
}

/// Run `<binary> version` with a timeout. Success means the binary is a
/// working Xray engine.
pub async fn probe_version(binary: &Path) -> Result<EngineVersion, EngineError> {
    if !binary.exists() {
        return Err(EngineError::NotFound(binary.to_path_buf()));
    }
    let mut cmd = tokio::process::Command::new(binary);
    cmd.arg("version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let output = tokio::time::timeout(Duration::from_secs(10), cmd.output())
        .await
        .map_err(|_| EngineError::NotReady("version probe timed out after 10s".into()))?
        .map_err(|e| EngineError::Spawn(format!("could not run the binary: {e}")))?;

    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push('\n');
    combined.push_str(&String::from_utf8_lossy(&output.stderr));

    parse_version_output(&combined).ok_or_else(|| {
        EngineError::NotReady("binary did not identify itself as an Xray engine".into())
    })
}

/// Semantic-ish comparison for Xray release tags (e.g. `v26.3.27`):
/// `true` when `latest` is strictly newer than `current`. Unparsable
/// inputs compare equal — the app never nags about data it cannot read.
pub fn is_version_newer(current: &str, latest: &str) -> bool {
    fn parse(v: &str) -> Option<Vec<u64>> {
        let trimmed = v.trim().trim_start_matches('v');
        let core = trimmed.split(['-', '+']).next()?;
        let parts: Option<Vec<u64>> = core.split('.').map(|p| p.parse::<u64>().ok()).collect();
        let parts = parts?;
        (!parts.is_empty()).then_some(parts)
    }
    match (parse(current), parse(latest)) {
        (Some(a), Some(b)) => {
            for index in 0..a.len().max(b.len()) {
                let x = a.get(index).copied().unwrap_or(0);
                let y = b.get(index).copied().unwrap_or(0);
                if x != y {
                    return y > x;
                }
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modern_output() {
        let v = parse_version_output(
            "Xray 26.3.27 (Xray, Penetrates Everything.) d2758a0 (go1.26.1 darwin/arm64)\nA unified platform for anti-censorship.\n",
        )
        .unwrap();
        assert_eq!(v.version, "26.3.27");
    }

    #[test]
    fn rejects_other_binaries() {
        assert!(parse_version_output("bash version 5.2").is_none());
        assert!(parse_version_output("Xray").is_none());
        assert!(parse_version_output("Xray not-a-version").is_none());
    }

    #[test]
    fn version_comparison_is_numeric_and_forgiving() {
        assert!(is_version_newer("v26.3.27", "v26.4.0"));
        assert!(is_version_newer("26.3.27", "26.10.0"), "two-digit minor");
        assert!(is_version_newer("v26.3.27", "v27.0.0"));
        assert!(!is_version_newer("v26.3.27", "v26.3.27"));
        assert!(!is_version_newer("v26.4.0", "v26.3.27"));
        assert!(!is_version_newer("v26.3.27", "v26.3.26"));
        assert!(
            is_version_newer("v26.3", "v26.3.1"),
            "shorter version is padded with zeros"
        );
        assert!(!is_version_newer("v26.3.27", "not-a-version"));
        assert!(!is_version_newer("garbage", "v26.4.0"));
    }
}
