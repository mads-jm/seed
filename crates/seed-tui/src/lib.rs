/// Test-facing library re-exports for seed-tui integration tests.
/// Exposes pure logic functions that don't require a running terminal.
pub mod command;
pub mod palette;
pub mod prestige;
pub mod view;

pub use command::{ParsedCommand, parse};
pub use palette::downgrade_color;
pub use view::orbit::braille_to_block;
