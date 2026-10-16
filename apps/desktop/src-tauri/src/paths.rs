//! Filesystem layout for everything KVN persists.
//!
//! All app-owned files live under the platform app-data directory:
//! - `state.json`  — profiles, subscriptions and settings (`0600`);
//! - `bin/`        — the verified engine binary;
//! - `runtime/`    — generated per-run engine config (`0600`);
//! - `logs/`       — engine logs and exported diagnostics.

use std::path::PathBuf;

use tauri::Manager;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub engine_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub state_file: PathBuf,
}

impl AppPaths {
    pub fn resolve(app: &tauri::AppHandle) -> Result<Self, String> {
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("cannot resolve the app data directory: {e}"))?;
        let paths = AppPaths {
            engine_dir: data_dir.join("bin"),
            runtime_dir: data_dir.join("runtime"),
            logs_dir: data_dir.join("logs"),
            state_file: data_dir.join("state.json"),
            data_dir,
        };
        for dir in [
            &paths.data_dir,
            &paths.engine_dir,
            &paths.runtime_dir,
            &paths.logs_dir,
        ] {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        Ok(paths)
    }

    pub fn engine_binary(&self) -> PathBuf {
        self.engine_dir
            .join(if cfg!(windows) { "xray.exe" } else { "xray" })
    }

    pub fn sysproxy_backup(&self) -> PathBuf {
        self.data_dir.join("sysproxy-backup.json")
    }
}
