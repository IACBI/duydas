//! Headless client SDK (Phase 1).
//!
//! Owns the gateway connection state machine, the durable outbox, REST calls and the local SQLite
//! cache. It talks to the UI only through bounded channels, so it is testable without a window.
