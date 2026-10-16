//! Typed errors for parsing, validation and config generation.
//!
//! Invariant: error messages never contain secret material (UUIDs,
//! passwords, full links). Where a field is named in an error, only the
//! field *name* is echoed, never its value.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, KvnError>;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum KvnError {
    #[error("unsupported URI scheme `{0}` (supported: vless, vmess, trojan, ss)")]
    UnsupportedScheme(String),

    #[error("malformed link: {0}")]
    MalformedLink(String),

    #[error("missing required field `{0}`")]
    MissingField(String),

    #[error("invalid port `{0}`")]
    InvalidPort(String),

    #[error("invalid value for `{field}`: {reason}")]
    InvalidValue { field: String, reason: String },

    #[error("unsupported parameter: {0}")]
    Unsupported(String),

    #[error("input is not a recognized profile link or subscription payload")]
    UnrecognizedInput,

    #[error("config generation failed: {0}")]
    Config(String),
}

impl KvnError {
    pub fn invalid(field: impl Into<String>, reason: impl Into<String>) -> Self {
        KvnError::InvalidValue {
            field: field.into(),
            reason: reason.into(),
        }
    }
}
