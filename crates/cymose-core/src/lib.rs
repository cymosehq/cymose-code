//! Everything both Cymose Code clients need and neither should own.
//!
//! The rule this crate lives by: no terminal, no rendering, no `println!`. It
//! returns values and emits events; the TUI and the VS Code extension decide
//! how those look. Anything that violates that ends up implemented twice, and
//! the two clients start disagreeing — which is what the shared-core design
//! exists to prevent.

pub mod agent;
pub mod api;
pub mod config;
pub mod context;
pub mod error;
pub mod router;
pub mod session;
pub mod store;
pub mod summarize;

pub use config::Config;
pub use error::{Error, Result};
pub use session::{Session, SessionStatus, Summary};
pub use store::Store;

/// Bumped only on a breaking change to the sidecar protocol. Additive fields
/// and new notifications do not bump it — see docs/sidecar-protocol.md.
pub const PROTOCOL_VERSION: u32 = 1;

/// This build's version, reported in the sidecar handshake so a mismatched
/// extension can say so instead of failing at the first call.
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
