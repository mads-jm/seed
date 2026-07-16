//! Wire-protocol re-export.
//!
//! The actual types and framing helpers live in the [`seed_wire`] crate so
//! that `seed-tui`, `seed-bridge`, and any future client can depend on them
//! without pulling in the daemon binary. This module preserves the historical
//! `seed_daemon::wire::*` import path used by integration tests.
pub use seed_wire::*;
