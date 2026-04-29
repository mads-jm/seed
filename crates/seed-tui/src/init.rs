/// `seed init` subcommand — scaffolds config.toml in the seed home directory.
use std::path::Path;

use anyhow::Result;
use seed_core::config::scaffold_default;

/// Run `seed init`: create the seed home directory and scaffold a default config.toml.
/// Prints a short status line to stdout.
pub fn run_init(seed_home: &Path) -> Result<()> {
    scaffold_default(seed_home)?;
    let config_path = seed_home.join("config.toml");
    println!(
        "created {} · edit to customize · run `seed` to begin",
        config_path.display()
    );
    Ok(())
}
