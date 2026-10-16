//! Downloading and installing the pinned engine build.
//!
//! This is the in-app "install / repair engine" action; release builds
//! normally start from the bundled sidecar instead (ADR 0005).
//!
//! The archive comes from the official Xray-core GitHub release and is
//! verified against the digest pinned in `core/xray.json` before anything
//! is extracted — a swapped release or a MITM cannot pass.
//!
//! The HTTP fetch happens here (the engine crate deliberately has no
//! network dependency); the verification and extraction stay in
//! `kvn_engine::install`.

use std::path::Path;

use kvn_engine::{asset_for_current_platform, download_url, extract_engine_binary, probe_version};

use crate::controller::EngineInfo;
use crate::eventlog::EventLog;
use crate::paths::AppPaths;

const USER_AGENT: &str = concat!(
    "kvn/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/zeroscrypt/kvn)"
);

pub async fn install_pinned(paths: &AppPaths, log: &EventLog) -> Result<EngineInfo, String> {
    let asset = asset_for_current_platform()
        .ok_or_else(|| "no pinned Xray build is published for this platform".to_owned())?;
    let url = download_url(asset);
    let zip_path = paths.runtime_dir.join(format!("{}.zip", asset.platform));

    log.push(
        "info",
        "engine",
        format!("downloading {url} (expected sha256 {})", asset.sha256),
    );

    let zip_for_task = zip_path.clone();
    let url_for_task = url.clone();
    tokio::task::spawn_blocking(move || download_to(&url_for_task, &zip_for_task))
        .await
        .map_err(|e| format!("download task failed: {e}"))??;

    let zip_for_extract = zip_path.clone();
    let engine_dir = paths.engine_dir.clone();
    let expected = asset.sha256.clone();
    let extracted = tokio::task::spawn_blocking(move || {
        extract_engine_binary(&zip_for_extract, &engine_dir, &expected)
    })
    .await
    .map_err(|e| format!("extract task failed: {e}"))?
    .map_err(|e| format!("the downloaded archive failed verification: {e}"))?;
    let _ = std::fs::remove_file(&zip_path);

    let version = probe_version(&extracted)
        .await
        .map_err(|e| format!("the installed binary did not pass the version probe: {e}"))?;
    log.push(
        "info",
        "engine",
        format!(
            "xray {} installed to {}",
            version.version,
            extracted.display()
        ),
    );

    Ok(EngineInfo {
        path: extracted.display().to_string(),
        version: version.version,
        source: "installed by KVN".to_owned(),
    })
}

fn download_to(url: &str, dest: &Path) -> Result<(), String> {
    let response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(|e| format!("download failed: {e}"))?;
    let mut file = std::fs::File::create(dest)
        .map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
    let mut reader = response.into_body().into_reader();
    let copied =
        std::io::copy(&mut reader, &mut file).map_err(|e| format!("download interrupted: {e}"))?;
    if copied == 0 {
        return Err("the download returned an empty archive".to_owned());
    }
    Ok(())
}
