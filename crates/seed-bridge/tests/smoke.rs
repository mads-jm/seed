//! Integration smoke test for `seed-bridge`.
//!
//! Marked `#[ignore]` so the normal `cargo test --workspace` stays hermetic —
//! this test binds the abstract `@seed-daemon.sock`, which would conflict with
//! any seedd already running on the developer's machine. Run it explicitly:
//!
//!   cargo test -p seed-bridge --test smoke -- --ignored --nocapture
//!
//! Asserts: bridge prints a `hello` line, then within 2s a `snapshot` line
//! that includes nine canonical traits.
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
#[ignore]
fn bridge_emits_hello_then_snapshot() {
    // 1. Spawn seedd in a temp SEED_HOME so we don't touch the real one.
    let tmp = tempfile::tempdir().expect("tempdir");
    let seed_home = tmp.path().to_path_buf();

    let workspace_root = workspace_root();
    let seedd_bin = workspace_root.join("target/debug/seedd");
    assert!(
        seedd_bin.exists(),
        "build seedd first: cargo build -p seed-daemon (looked at {})",
        seedd_bin.display()
    );

    let mut seedd = Command::new(&seedd_bin)
        .arg("--foreground")
        .env("SEED_HOME", &seed_home)
        .env("SEED_LOG", "error")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn seedd");

    // Give seedd a moment to bind the socket.
    std::thread::sleep(Duration::from_millis(500));

    // 2. Spawn the bridge.
    let bridge_bin = workspace_root.join("target/debug/seed-bridge");
    assert!(
        bridge_bin.exists(),
        "build seed-bridge first: cargo build -p seed-bridge (looked at {})",
        bridge_bin.display()
    );

    let mut bridge = Command::new(&bridge_bin)
        .env("SEED_HOME", &seed_home)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn bridge");

    let stdout = bridge.stdout.take().expect("stdout pipe");
    let reader = BufReader::new(stdout);

    let mut saw_hello = false;
    let mut saw_snapshot = false;
    let deadline = Instant::now() + Duration::from_secs(5);

    for line in reader.lines() {
        if Instant::now() > deadline {
            break;
        }
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("hello") => {
                saw_hello = true;
            }
            Some("snapshot") => {
                saw_snapshot = true;
                let traits = v
                    .pointer("/state/traits")
                    .expect("snapshot.state.traits missing");
                let trait_keys: Vec<&str> = traits
                    .as_object()
                    .expect("traits is object")
                    .keys()
                    .map(|s| s.as_str())
                    .collect();
                assert_eq!(trait_keys.len(), 9, "expected 9 traits, got {trait_keys:?}");
                break;
            }
            _ => {}
        }
    }

    // 3. Tear down.
    let _ = bridge.kill();
    let _ = bridge.wait();
    let _ = seedd.kill();
    let _ = seedd.wait();

    assert!(saw_hello, "bridge did not emit a hello frame within 5s");
    assert!(
        saw_snapshot,
        "bridge did not emit a snapshot frame within 5s"
    );
}

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is the bridge crate; walk up to workspace root.
    let me = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    me.parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}
