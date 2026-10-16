//! Typed engine errors. Messages never contain credentials: config content
//! is referenced by path, engine output is truncated and never echoed with
//! the config embedded.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("engine binary not found at {0}")]
    NotFound(PathBuf),

    #[error("failed to start the engine process: {0}")]
    Spawn(String),

    #[error("the engine rejected the generated config: {0}")]
    ConfigRejected(String),

    #[error("engine did not become ready: {0}")]
    NotReady(String),

    #[error("health check failed: {0}")]
    Health(String),

    #[error("traffic stats query failed: {0}")]
    Stats(String),

    #[error("download failed: {0}")]
    Download(String),

    #[error("checksum mismatch for {file}: expected {expected}, got {actual}")]
    Checksum {
        file: String,
        expected: String,
        actual: String,
    },

    #[error("failed to extract the engine archive: {0}")]
    Extract(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
