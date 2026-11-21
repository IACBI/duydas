//! Duydaş wire protocol.
//!
//! The schema lives in `proto/duydas/v1/`. Rust types will be generated with prost by
//! `cargo xtask proto-gen` and committed, so building never needs `protoc` (ADR 0004).

/// Protocol major version spoken by this build. Clients and servers support version skew of
/// one release (N-1); see `docs/architecture.md`.
pub const PROTOCOL_VERSION: u32 = 1;
