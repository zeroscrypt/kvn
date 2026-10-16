//! # kvn-core
//!
//! Platform-independent domain logic for KVN, a desktop client for
//! 3x-ui / Xray-core servers:
//!
//! - [`profile`] — the typed profile model (protocol, transport, security);
//! - [`uri`] — share-link and subscription-payload parsing (VLESS, VMess,
//!   Trojan, Shadowsocks), with strict validation and clear errors;
//! - [`xray`] — generation of a complete Xray client configuration from a
//!   profile, with explicit notes about anything that was dropped or that
//!   weakens security;
//! - [`state`] — the connection state machine (pure, testable, no I/O);
//! - [`secret`] — a `Secret` string wrapper that cannot leak through
//!   `Debug`/`Display`, plus link redaction helpers for logs.
//!
//! This crate has no platform or UI dependencies on purpose: everything
//! here is unit-tested in isolation.

pub mod error;
pub mod profile;
pub mod secret;
pub mod state;
pub mod uri;
pub mod xray;

pub use error::{KvnError, Result};
pub use profile::{
    Endpoint, Header, Profile, ProfileId, ProfileSource, Protocol, ProtocolSpec, RealityConfig,
    Security, TcpHeader, TlsConfig, Transport,
};
pub use secret::Secret;
