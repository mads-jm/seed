/// End-to-end smoke test for the `seedd` daemon.
///
/// Steps:
/// 1. Set SEED_HOME to a tempdir.
/// 2. Spawn `seedd --foreground` as a child process.
/// 3. Wait up to 2s for the IPC socket to become available (probe via Ping).
/// 4. Connect a raw IPC client using `seed_daemon::wire` framing.
/// 5. Send Subscribe → expect Snapshot response.
/// 6. Send Complete("water") → expect Response { Ok }.
/// 7. Expect a StateDiff containing a ReminderCompleted event.
/// 8. Read events.jsonl and assert the completion event is written.
/// 9. Send Shutdown → expect daemon process to exit within 2s.
///
/// The test is marked `#[ignore]` by default because it requires the `seedd`
/// binary to be built and accessible via `CARGO_BIN_EXE_seedd`, which is only
/// reliable in `cargo test` (not `cargo nextest` without `--cargo-quiet`).
/// Run explicitly with:
///   cargo test --test smoke -- --ignored
///
/// # Isolation
///
/// Every socket name here is derived from *this test's* temp `SEED_HOME` via
/// [`test_socket_name`], never from the ambient environment. That matters more
/// than it looks: `SEED_HOME` is set on the spawned child only, so the test
/// process's own environment still points at the default home. Resolving the
/// name from the environment (as `seed_daemon::ipc::socket_name()` does, by
/// design) would therefore target the developer's *real* daemon — and this test
/// completes a reminder and then shuts the daemon down. It has done exactly
/// that. `test_socket_name` asserts it never happens again.
use std::path::Path;
use std::time::{Duration, Instant};

use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::{GenericNamespaced, Name, ToNsName};
use seed_core::{EventEnvelope, ReminderId, default_seed_home};
use seed_daemon::wire::{
    Action, Message, ResponseResult, read_frame, socket_name_for, write_frame,
};
use tokio::io::BufReader;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The socket serving `seed_home`, with a hard stop if that resolves to the
/// socket of the *default* home.
///
/// The default home's daemon is the developer's live one, holding real data.
/// A mis-derived name here is not a failed assertion, it is silent data loss —
/// so refuse to proceed rather than connect and find out.
fn test_socket_name(seed_home: &Path) -> Name<'static> {
    let raw = socket_name_for(seed_home);
    assert_ne!(
        raw,
        socket_name_for(&default_seed_home()),
        "refusing to run: this test derived the DEFAULT seed home's socket name \
         ({raw}) from temp home {}. That is the real daemon — the test would \
         complete a reminder against live data and then shut it down.",
        seed_home.display(),
    );
    raw.to_ns_name::<GenericNamespaced>()
        .expect("build socket name")
}

/// Kills the daemon on drop, however the test ends.
///
/// Without this, a panic between spawn and the Shutdown step leaks a live
/// `seedd`; because the child inherits the test harness's stdout, that also
/// wedges `cargo test`, which waits on a pipe the orphan holds open forever.
struct ChildGuard(std::process::Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        // Both calls no-op with an error once the test has reaped it normally.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Probe whether the daemon socket is up. Returns true if Pong is received.
///
/// The daemon sends a `Hello` frame on connect before any client message.
/// We drain that first, then send Ping and wait for Pong.
async fn probe_socket(seed_home: &Path) -> bool {
    let name = test_socket_name(seed_home);
    use interprocess::local_socket::tokio::Stream;
    let conn = match Stream::connect(name).await {
        Ok(c) => c,
        Err(_) => return false,
    };
    let (rd, mut wr) = tokio::io::split(conn);
    let mut rd = BufReader::new(rd);

    // Drain the Hello frame the daemon sends immediately on connect.
    match tokio::time::timeout(Duration::from_millis(500), read_frame(&mut rd)).await {
        Ok(Ok(Some(Message::Hello { .. }))) => {}
        _ => return false, // unexpected or timeout — not ready yet
    }

    // Now send Ping and wait for Pong.
    if write_frame(&mut wr, &Message::Ping).await.is_err() {
        return false;
    }
    matches!(
        tokio::time::timeout(Duration::from_secs(1), read_frame(&mut rd)).await,
        Ok(Ok(Some(Message::Pong)))
    )
}

/// Wait up to `timeout` for the daemon socket to answer a Ping.
async fn wait_for_socket(timeout: Duration, seed_home: &Path) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if probe_socket(seed_home).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// The tripwire must actually fire — otherwise it is decoration.
///
/// Not `#[ignore]`d: this is the guard that keeps the ignored test below from
/// ever again pointing at a real daemon, so it earns its keep in every run. It
/// touches no sockets and no daemon.
#[test]
#[should_panic(expected = "refusing to run")]
fn test_socket_name_refuses_the_default_home() {
    let _ = test_socket_name(&default_seed_home());
}

// ---------------------------------------------------------------------------
// Smoke test
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires seedd binary; run with: cargo test --test smoke -- --ignored"]
async fn smoke_daemon_complete_and_shutdown() {
    let started = Instant::now();

    // ---- 1. Temp SEED_HOME ------------------------------------------------
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let seed_home = tmp.path().to_path_buf();

    // ---- 2. Spawn seedd ---------------------------------------------------
    let seedd_bin = env!("CARGO_BIN_EXE_seedd");
    // SEED_HOME is set on the child only — the test process keeps its own
    // environment, which is why every socket name below is derived explicitly
    // from `seed_home` rather than resolved from the ambient env.
    let mut child = ChildGuard(
        std::process::Command::new(seedd_bin)
            .arg("--foreground")
            .env("SEED_HOME", &seed_home)
            .env("SEED_LOG", "warn") // suppress noise in test output
            .spawn()
            .expect("failed to spawn seedd"),
    );

    // ---- 3. Wait for socket -----------------------------------------------
    // ChildGuard reaps the daemon from here on, including on panic.
    let up = wait_for_socket(Duration::from_secs(5), &seed_home).await;
    assert!(up, "seedd socket did not become available within 5s");

    // ---- 4. Connect -------------------------------------------------------
    let name = test_socket_name(&seed_home);
    use interprocess::local_socket::tokio::Stream;
    let conn = Stream::connect(name)
        .await
        .expect("connect to seedd socket");
    let (rd, mut wr) = tokio::io::split(conn);
    let mut rd = BufReader::new(rd);

    // Drain the Hello frame sent on connect.
    let hello = read_frame(&mut rd)
        .await
        .expect("read Hello")
        .expect("Hello frame");
    assert!(
        matches!(hello, Message::Hello { .. }),
        "expected Hello on connect, got {hello:?}"
    );

    // ---- 5. Subscribe → Snapshot ------------------------------------------
    write_frame(
        &mut wr,
        &Message::Request {
            id: 1,
            action: Action::Subscribe,
        },
    )
    .await
    .expect("write Subscribe");

    // Snapshot arrives directly (not wrapped in Response per protocol design).
    let snap_msg = read_frame(&mut rd)
        .await
        .expect("read Snapshot")
        .expect("Snapshot frame");
    assert!(
        matches!(snap_msg, Message::Snapshot { .. }),
        "expected Snapshot after Subscribe, got {snap_msg:?}"
    );

    // ---- 6. Complete("water") → Response { Ok } ---------------------------
    write_frame(
        &mut wr,
        &Message::Request {
            id: 2,
            action: Action::Complete {
                reminder_id: ReminderId("water".into()),
            },
        },
    )
    .await
    .expect("write Complete");

    // Collect frames until we see the Response for id=2.
    // A StateDiff may arrive first (broadcast to all subscribers).
    let mut got_ok_response = false;
    let mut got_state_diff = false;
    let mut diff_envelopes: Vec<EventEnvelope> = Vec::new();

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && (!got_ok_response || !got_state_diff) {
        let frame = tokio::time::timeout(Duration::from_secs(2), read_frame(&mut rd))
            .await
            .expect("read frame timeout")
            .expect("read frame error")
            .expect("EOF before response");

        match frame {
            Message::Response {
                id: 2,
                result: ResponseResult::Ok { .. },
            } => {
                got_ok_response = true;
            }
            Message::StateDiff { events } => {
                diff_envelopes.extend(events);
                got_state_diff = true;
            }
            // Ignore anything else (e.g., stray Snapshot from broadcast).
            _ => {}
        }
    }

    assert!(
        got_ok_response,
        "never received Response {{ Ok }} for Complete(water)"
    );
    assert!(
        got_state_diff,
        "never received StateDiff after Complete(water)"
    );

    // ---- 7. StateDiff contains ReminderCompleted --------------------------
    let has_completed = diff_envelopes
        .iter()
        .any(|env| env.kind == "seed.reminder.completed");
    assert!(
        has_completed,
        "StateDiff envelopes did not contain seed.reminder.completed; got: {diff_envelopes:#?}"
    );

    // ---- 8. events.jsonl on disk has the event ----------------------------
    let events_path = seed_home.join("events.jsonl");
    // Give the daemon a moment to fsync (it fsyncs synchronously on append, so
    // if Response was received the write is already committed — but wait a tick).
    tokio::time::sleep(Duration::from_millis(50)).await;
    let contents = std::fs::read_to_string(&events_path)
        .expect("events.jsonl should exist after a Complete action");
    assert!(
        contents.contains("seed.reminder.completed"),
        "events.jsonl did not contain seed.reminder.completed:\n{contents}"
    );
    assert!(
        contents.contains("\"water\""),
        "events.jsonl did not contain reminder_id \"water\":\n{contents}"
    );

    // ---- 9. Shutdown → daemon exits ---------------------------------------
    write_frame(
        &mut wr,
        &Message::Request {
            id: 3,
            action: Action::Shutdown,
        },
    )
    .await
    .expect("write Shutdown");

    // Wait for process exit (up to 3s).
    let shutdown_start = Instant::now();
    let exit_status = loop {
        match child.0.try_wait().expect("try_wait") {
            Some(status) => break status,
            None => {
                // ChildGuard kills it on the way out of this panic.
                assert!(
                    shutdown_start.elapsed() <= Duration::from_secs(3),
                    "seedd did not exit within 3s after Shutdown request"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    };
    assert!(
        exit_status.success(),
        "seedd exited with non-zero status: {exit_status}"
    );

    // ---- Summary ----------------------------------------------------------
    let elapsed_ms = started.elapsed().as_millis();
    println!("smoke test PASSED in {elapsed_ms}ms");
}
