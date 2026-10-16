//! Engine distribution: bundled sidecar, verified installation, digests.
//!
//! ADR 0005: release artifacts bundle a pinned Xray-core as a Tauri sidecar
//! (`externalBin`), so a fresh install works without touching the network.
//! The in-app installer remains as the update/repair path for that engine,
//! and a user-supplied binary always wins (ADR 0003).
//!
//! Whatever the source, an archive is verified against the SHA-256 digest
//! pinned in `core/xray.json` (embedded via [`crate::manifest`]) before
//! anything is extracted — a swapped release cannot pass.

use std::path::{Path, PathBuf};

use crate::error::EngineError;

pub use crate::manifest::{
    asset_for_current_platform, current_platform, download_url, release, EngineAsset,
};

/// The sidecar engine that ships inside the app bundle: `xray` next to the
/// main executable (Tauri `externalBin` places it in `Contents/MacOS/` on
/// macOS and beside the `.exe` on Windows). `None` when this executable has
/// no sidecar next to it (test binaries, plain `cargo run` without fetched
/// sidecar binaries).
pub fn bundled_engine_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = exe
        .parent()?
        .join(if cfg!(windows) { "xray.exe" } else { "xray" });
    candidate.is_file().then_some(candidate)
}

/// SHA-256 of a file, as lowercase hex.
pub fn sha256_file(path: &Path) -> Result<String, EngineError> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(to_hex(&hasher.finalize()))
}

pub fn verify_sha256(path: &Path, expected_hex: &str) -> Result<(), EngineError> {
    let actual = sha256_file(path)?;
    let expected = expected_hex.trim().to_ascii_lowercase();
    if actual == expected {
        Ok(())
    } else {
        Err(EngineError::Checksum {
            file: path.display().to_string(),
            expected,
            actual,
        })
    }
}

/// Verify the archive digest and extract only the `xray` binary into
/// `dest_dir` (written atomically; executable bit set on unix).
pub fn extract_engine_binary(
    zip_path: &Path,
    dest_dir: &Path,
    expected_sha256: &str,
) -> Result<PathBuf, EngineError> {
    verify_sha256(zip_path, expected_sha256)?;

    let file = std::fs::File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| EngineError::Extract(e.to_string()))?;
    std::fs::create_dir_all(dest_dir)?;

    let binary_name = if cfg!(windows) { "xray.exe" } else { "xray" };
    let dest = dest_dir.join(binary_name);
    let tmp = dest_dir.join(format!("{binary_name}.part"));
    let _ = std::fs::remove_file(&tmp);

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| EngineError::Extract(e.to_string()))?;
        let entry_name = entry
            .name()
            .map_err(|e| EngineError::Extract(e.to_string()))?
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .to_owned();
        // Never trust archive paths: we look the entry up by bare file
        // name and write to our own destination, so zip-slip is impossible.
        if entry_name != "xray" && entry_name != "xray.exe" {
            continue;
        }
        if entry.is_dir() {
            continue;
        }
        let mut out = std::fs::File::create(&tmp)?;
        std::io::copy(&mut entry, &mut out)?;
        drop(out);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&tmp, &dest)?;
        return Ok(dest);
    }
    Err(EngineError::Extract(
        "the archive does not contain an xray binary".into(),
    ))
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_known_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.bin");
        std::fs::write(&path, b"hello").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        verify_sha256(
            &path,
            "2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824",
        )
        .unwrap();
        assert!(verify_sha256(&path, "00").is_err());
    }

    #[test]
    fn extract_verifies_digest_and_writes_binary() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("engine.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("README.md", options).unwrap();
            writer.write_all(b"docs").unwrap();
            writer.start_file("xray", options).unwrap();
            writer.write_all(b"#!/bin/false\n").unwrap();
            writer.finish().unwrap();
        }
        let digest = sha256_file(&zip_path).unwrap();

        // Wrong digest → refuses to extract.
        assert!(extract_engine_binary(&zip_path, dir.path(), "deadbeef").is_err());

        let binary = extract_engine_binary(&zip_path, dir.path(), &digest).unwrap();
        let expected_name = if cfg!(windows) { "xray.exe" } else { "xray" };
        assert_eq!(binary.file_name().unwrap(), expected_name);
        assert_eq!(std::fs::read(&binary).unwrap(), b"#!/bin/false\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&binary).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "engine binary must be executable");
        }
    }

    #[test]
    fn extract_rejects_archives_without_engine() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("not-engine.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("something-else.txt", options).unwrap();
            std::io::Write::write_all(&mut writer, b"nope").unwrap();
            writer.finish().unwrap();
        }
        let digest = sha256_file(&zip_path).unwrap();
        assert!(extract_engine_binary(&zip_path, dir.path(), &digest).is_err());
    }
}
