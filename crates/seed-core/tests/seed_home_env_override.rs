/// Env-var override test for `paths::seed_home()`.
///
/// Isolated in its own integration-test file so it runs as a separate process.
/// Integration test binaries are single-threaded by default — no risk of a
/// concurrent test reading SEED_HOME while this test mutates it.
use std::path::PathBuf;

use seed_core::paths::seed_home;

#[test]
fn seed_home_env_override() {
    // SAFETY: This is the only test in this file. Integration test binaries
    // compile each `tests/*.rs` file into a separate executable. No other
    // thread reads SEED_HOME in this process.
    unsafe { std::env::set_var("SEED_HOME", "/custom/seed_home") };
    let p = seed_home();
    unsafe { std::env::remove_var("SEED_HOME") };
    assert_eq!(p, PathBuf::from("/custom/seed_home"));
}
