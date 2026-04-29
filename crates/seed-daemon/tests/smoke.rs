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
use std::time::{Duration, Instant};

use interprocess::local_socket::tokio::prelude::*;
use seed_core::{EventEnvelope, ReminderId};
use seed_daemon::ipc::socket_name;
use seed_daemon::wire::{Action, Message, ResponseResult, read_frame, write_frame};
use tokio::io::BufReader;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Probe whether the daemon socket is up. Returns true if Pong is received.
///
/// The daemon sends a `Hello` frame on connect before any client message.
/// We drain that first, then send Ping and wait for Pong.
async fn probe_socket() -> bool {
    let name = match socket_name() {
        Ok(n) => n,
        Err(_) => return false,
    };
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
async fn wait_for_socket(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if probe_socket().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
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
    let mut child = std::process::Command::new(seedd_bin)
        .arg("--foreground")
        .env("SEED_HOME", &seed_home)
        .env("SEED_LOG", "warn") // suppress noise in test output
        .spawn()
        .expect("failed to spawn seedd");

    // ---- 3. Wait for socket -----------------------------------------------
    let up = wait_for_socket(Duration::from_secs(5)).await;
    if !up {
        child.kill().ok();
        panic!("seedd socket did not become available within 5s");
    }

    // ---- 4. Connect -------------------------------------------------------
    let name = socket_name().expect("socket name");
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
        match child.try_wait().expect("try_wait") {
            Some(status) => break status,
            None => {
                if shutdown_start.elapsed() > Duration::from_secs(3) {
                    child.kill().ok();
                    panic!("seedd did not exit within 3s after Shutdown request");
                }
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
