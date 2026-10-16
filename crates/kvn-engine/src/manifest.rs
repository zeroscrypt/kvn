//! The engine pin manifest — `core/xray.json`, embedded at compile time.
//!
//! Single source of truth for the pinned Xray-core release, the per-platform
//! archive file names, their SHA-256 digests (from the release `.dgst`
//! files) and the Rust target triple each archive maps to. The app
//! ([`crate::install`]), `scripts/fetch-xray.sh`, `scripts/update-core-pin.py`
//! and the CI workflows all read this same file — bumping the engine is a
//! one-file change.

use std::sync::OnceLock;

use serde::Deserialize;

const MANIFEST_JSON: &str = include_str!("../../../core/xray.json");

#[derive(Debug, Deserialize)]
pub struct Manifest {
    /// GitHub `owner/repo` the pinned release lives in.
    pub repository: String,
    /// Release tag on GitHub, e.g. `v26.3.27`.
    pub release: String,
    pub assets: Vec<EngineAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EngineAsset {
    /// Platform tag (see [`current_platform`]).
    pub platform: String,
    /// Rust target triple the archive corresponds to; names the sidecar
    /// binary (`xray-<triple>`) for Tauri `externalBin`.
    pub triple: String,
    /// File name in the GitHub release.
    pub file: String,
    /// SHA-256 of the archive, pinned from the release's `.dgst` file.
    pub sha256: String,
}

fn manifest() -> &'static Manifest {
    static MANIFEST: OnceLock<Manifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        serde_json::from_str(MANIFEST_JSON)
            .expect("core/xray.json must be valid (checked by unit tests)")
    })
}

/// Pinned release tag, e.g. `v26.3.27`.
pub fn release() -> &'static str {
    &manifest().release
}

/// GitHub repository (`owner/repo`) of the pinned release.
pub fn repository() -> &'static str {
    &manifest().repository
}

pub fn assets() -> &'static [EngineAsset] {
    &manifest().assets
}

/// Platform tag for the running build, or `None` when no pin exists for it
/// (the UI then asks for a user-provided binary).
pub fn current_platform() -> Option<&'static str> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        return Some("macos-arm64");
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return Some("macos-amd64");
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        return Some("linux-amd64");
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        return Some("linux-arm64");
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return Some("windows-amd64");
    }
    #[allow(unreachable_code)]
    None
}

pub fn asset_for_platform(platform: &str) -> Option<&'static EngineAsset> {
    assets().iter().find(|a| a.platform == platform)
}

pub fn asset_for_current_platform() -> Option<&'static EngineAsset> {
    current_platform().and_then(asset_for_platform)
}

pub fn asset_for_triple(triple: &str) -> Option<&'static EngineAsset> {
    assets().iter().find(|a| a.triple == triple)
}

pub fn download_url(asset: &EngineAsset) -> String {
    format!(
        "https://github.com/{}/releases/download/{}/{}",
        repository(),
        release(),
        asset.file
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_valid_and_complete() {
        assert!(release().starts_with('v'));
        assert_eq!(repository(), "XTLS/Xray-core");
        assert_eq!(assets().len(), 5, "five platforms are pinned");
        for asset in assets() {
            assert_eq!(
                asset.sha256.len(),
                64,
                "{}: SHA-256 must be hex",
                asset.platform
            );
            assert!(asset.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(asset.file.ends_with(".zip"));
            assert!(
                asset.triple.split('-').count() >= 3,
                "{}: must carry a full target triple",
                asset.platform
            );
        }
        let mac = asset_for_triple("aarch64-apple-darwin").expect("macos arm64 pin");
        assert_eq!(mac.platform, "macos-arm64");
        assert_eq!(mac.file, "Xray-macos-arm64-v8a.zip");
    }

    #[test]
    fn download_url_points_at_the_pinned_release() {
        let asset = asset_for_current_platform().expect("test host must have a pinned asset");
        let url = download_url(asset);
        assert!(url.starts_with("https://github.com/XTLS/Xray-core/releases/download/v"));
        assert!(url.ends_with(&asset.file));
        assert!(url.contains(release()));
    }
}
