//! # kvn-engine
//!
//! Management layer for the Xray-core process used by KVN:
//!
//! - [`manifest`] — the engine pin (`core/xray.json`): release, per-platform
//!   archives, SHA-256 digests and target triples; embedded at compile time;
//! - [`process`] — spawn, supervise and stop the engine process;
//! - [`version`] — identify a usable engine binary (`xray version`) and
//!   compare release tags;
//! - [`health`] — readiness (proxy port accepts) and *real traffic* checks
//!   through the tunnel (SOCKS5 CONNECT + HTTP request), never just
//!   "process is running";
//! - [`stats`] — traffic counters via the engine's own stats API;
//! - [`install`] — the bundled sidecar, verified installation from official
//!   release archives (SHA-256 pinned), user-supplied binaries.
//!
//! The crate has no UI or platform-shell dependencies; platform-specific
//! bits (process signalling) are small `cfg`-gated sections.

pub mod error;
pub mod health;
pub mod install;
pub mod manifest;
pub mod process;
pub mod stats;
pub mod version;

pub use error::EngineError;
pub use health::{check_through_socks, wait_for_port, HealthTarget};
pub use install::{
    asset_for_current_platform, bundled_engine_path, current_platform, download_url,
    extract_engine_binary, release, verify_sha256, EngineAsset,
};
pub use process::{spawn_engine, validate_config, EngineProcess, SpawnOptions};
pub use stats::{parse_stats_json, query_stats, TrafficSnapshot};
pub use version::{is_version_newer, parse_version_output, probe_version, EngineVersion};

/// Truncate `text` to at most `max_bytes`, never splitting a UTF-8
/// character; a `…` marks the cut. Engine output can contain multi-byte
/// characters (localized error messages), so slicing by byte index alone
/// would panic mid-character.
pub(crate) fn truncate_chars(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::truncate_chars;

    #[test]
    fn truncate_chars_never_splits_a_character() {
        assert_eq!(truncate_chars("ok", 10), "ok");
        assert_eq!(truncate_chars("ровно", 10), "ровно");
        // 4-byte prefix of Cyrillic text, then the ellipsis.
        assert_eq!(truncate_chars("ошибка при запуске", 5), "ош…");
        // Degenerate limit below one character yields just the ellipsis.
        assert_eq!(truncate_chars("要", 1), "…");
    }
}
