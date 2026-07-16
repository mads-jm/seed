/// Path helpers for seed's home directory layout.
/// Honors `SEED_HOME` env var; falls back to `~/.seed`.
use std::path::{Path, PathBuf};

/// Resolve the seed home directory. Honors `SEED_HOME` env var.
/// Falls back to `<home_dir>/.seed` or `./.seed` if home is unavailable.
pub fn seed_home() -> PathBuf {
    std::env::var("SEED_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_seed_home())
}

/// The seed home used when `SEED_HOME` is unset: `<home_dir>/.seed`, or
/// `./.seed` if the home directory is unavailable.
///
/// Deliberately ignores `SEED_HOME` — callers need to recognise the default
/// home *as* the default even when it was named explicitly via the env var
/// (`SEED_HOME=~/.seed` and an unset `SEED_HOME` must resolve identically).
pub fn default_seed_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".seed")
}

/// `<home>/events.jsonl` — append-only event log.
pub fn events_path(home: &Path) -> PathBuf {
    home.join("events.jsonl")
}

/// `<home>/snapshot.json` — periodic derived state snapshot.
pub fn snapshot_path(home: &Path) -> PathBuf {
    home.join("snapshot.json")
}

/// `<home>/config.toml` — optional user config.
pub fn config_path(home: &Path) -> PathBuf {
    home.join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_path_appends_correctly() {
        let home = Path::new("/tmp/seed_test");
        assert_eq!(
            events_path(home),
            PathBuf::from("/tmp/seed_test/events.jsonl")
        );
    }

    #[test]
    fn snapshot_path_appends_correctly() {
        let home = Path::new("/tmp/seed_test");
        assert_eq!(
            snapshot_path(home),
            PathBuf::from("/tmp/seed_test/snapshot.json")
        );
    }

    #[test]
    fn config_path_appends_correctly() {
        let home = Path::new("/tmp/seed_test");
        assert_eq!(
            config_path(home),
            PathBuf::from("/tmp/seed_test/config.toml")
        );
    }
    // seed_home_env_override is in tests/seed_home_env_override.rs (its own
    // integration-test file) so it runs as an isolated process, avoiding any
    // race with other tests that might read SEED_HOME in the same binary.
}
