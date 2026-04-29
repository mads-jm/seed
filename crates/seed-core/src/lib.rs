//! `seed-core` — pure domain logic for the seed wellness companion.
//!
//! Provides: OSRS XP curve, reminder catalog (9 categories / 20 reminders),
//! tier system, append-only event log, state fold, and config loader.
//! No I/O — clock and filesystem are injected at call sites.

pub mod config;
pub mod domain;
pub mod events;
pub mod glyph;
pub mod levels;
pub mod paths;
pub mod state;

pub use config::*;
pub use domain::*;
pub use events::*;
pub use levels::*;
pub use paths::*;
pub use state::*;

/// Returns the crate version from the package manifest.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
