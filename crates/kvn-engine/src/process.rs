//! Engine process lifecycle.
//!
//! - [`validate_config`] runs `xray run -test -config <path>` — the engine
//!   itself validates the generated config before we ever start a tunnel;
//! - [`spawn_engine`] starts the engine as a child process with its own
//!   log files, never inheriting the app's stdio;
//! - [`EngineProcess::stop`] performs a graceful shutdown (SIGTERM with a
//!   grace period, then hard kill) so no zombie engine survives the app.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::EngineError;

#[derive(Debug, Clone)]
pub struct SpawnOptions {
    pub binary: PathBuf,
    pub config_path: PathBuf,
    /// Engine stdout/stderr are appended here (pre-config-load errors land
    /// here; normal engine logs go to the file configured in the config).
    pub stderr_log: Option<PathBuf>,
    pub work_dir: Option<PathBuf>,
}

#[derive(Debug)]
pub struct EngineProcess {
    child: tokio::process::Child,
    pid: u32,
}

impl EngineProcess {
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Returns `Some(status)` exactly once the process has exited.
    pub async fn try_exit(&mut self) -> Option<std::process::ExitStatus> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status),
            _ => None,
        }
    }

    /// Immediate hard kill, for panic-safe cleanup guards.
    pub fn kill_now(&mut self) {
        let _ = self.child.start_kill();
    }

    /// Graceful stop: SIGTERM, wait up to `grace`, then hard kill.
    pub async fn stop(mut self, grace: Duration) -> Result<(), EngineError> {
        #[cfg(unix)]
        {
            // SAFETY: kill(2) with a pid we own; errors are non-fatal here
            // because the process may already be gone.
            unsafe {
                libc::kill(self.pid as i32, libc::SIGTERM);
            }
            match tokio::time::timeout(grace, self.child.wait()).await {
                Ok(_) => Ok(()),
                Err(_) => {
                    let _ = self.child.start_kill();
                    let _ = self.child.wait().await;
                    Ok(())
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = grace;
            let _ = self.child.start_kill();
            let _ = self.child.wait().await;
            Ok(())
        }
    }
}

/// Start the engine with `run -config <config>`. The process gets its own
/// session (unix) so it can be cleaned up reliably even if the parent dies
/// unexpectedly in development; stdout is discarded, stderr goes to the
/// optional log file.
pub async fn spawn_engine(opts: &SpawnOptions) -> Result<EngineProcess, EngineError> {
    if !opts.binary.exists() {
        return Err(EngineError::NotFound(opts.binary.clone()));
    }
    let mut cmd = tokio::process::Command::new(&opts.binary);
    cmd.arg("run")
        .arg("-config")
        .arg(&opts.config_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null());

    match &opts.stderr_log {
        Some(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            let clone = file.try_clone()?;
            cmd.stderr(std::process::Stdio::from(file));
            cmd.stdout(std::process::Stdio::from(clone));
        }
        None => {
            cmd.stderr(std::process::Stdio::null());
        }
    }
    if let Some(dir) = &opts.work_dir {
        cmd.current_dir(dir);
    }

    #[cfg(unix)]
    {
        // SAFETY: setsid() is async-signal-safe and has no memory effects.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    let child = cmd
        .spawn()
        .map_err(|e| EngineError::Spawn(format!("failed to execute engine: {e}")))?;
    let pid = child
        .id()
        .ok_or_else(|| EngineError::Spawn("engine exited immediately".into()))?;
    Ok(EngineProcess { child, pid })
}

/// Validate a config file with the engine itself (`run -test`). This is the
/// authoritative gate before starting a tunnel: if this fails, the error
/// text (truncated) is surfaced to the user.
pub async fn validate_config(binary: &Path, config_path: &Path) -> Result<(), EngineError> {
    let mut cmd = tokio::process::Command::new(binary);
    cmd.arg("run")
        .arg("-test")
        .arg("-config")
        .arg(config_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let output = tokio::time::timeout(Duration::from_secs(20), cmd.output())
        .await
        .map_err(|_| EngineError::NotReady("config validation timed out".into()))?
        .map_err(|e| EngineError::Spawn(format!("could not run the engine: {e}")))?;

    if output.status.success() {
        return Ok(());
    }
    let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
    if text.trim().is_empty() {
        text = String::from_utf8_lossy(&output.stdout).into_owned();
    }
    Err(EngineError::ConfigRejected(truncate(&text, 2000)))
}

fn truncate(text: &str, max: usize) -> String {
    crate::truncate_chars(text.trim(), max)
}
