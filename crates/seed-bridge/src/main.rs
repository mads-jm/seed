//! `seed-bridge` — line-oriented stdout adapter for `seedd`.
//!
//! WHAT  Subscribes to the seed daemon's IPC socket and re-emits each
//!       incoming frame as one JSON object per line on stdout. Health
//!       transitions are surfaced as their own `{"type":"health",…}` lines
//!       so downstream UIs can show a disconnect indicator without polling.
//!
//! WHY   Quickshell (and any other line-oriented consumer — a status bar,
//!       a tmux statusline, a polybar applet) can't easily speak the
//!       length-prefixed binary framing the daemon uses. The bridge owns
//!       reconnect/backoff, hides the framing, and keeps unknown event
//!       kinds round-tripping verbatim so new daemon events never break
//!       downstream consumers — they just receive richer payloads.
//!
//! HOW   `tokio` runtime, one task. Connect via `interprocess` to the
//!       abstract-namespace socket, wait for the daemon's `Hello`, send a
//!       `Subscribe`, then forward every inbound frame to stdout. On any
//!       socket error: emit `{"type":"health","connected":false}`,
//!       backoff (200ms → 400 → 800 → 1600 → cap 5000), retry. On stdin
//!       receiving `quit\n`, exit cleanly.
//!
//! Output protocol (one JSON object per line on stdout):
//!
//!   {"type":"hello",    "bridge_version":"0.1.0", "v":1}
//!   {"type":"health",   "connected":true}
//!   {"type":"snapshot", "state":{ /* seed_core::State verbatim */ }}
//!   {"type":"diff",     "events":[ /* EventEnvelope[] */ ]}
//!
//! Tracing goes to stderr only — stdout is reserved for the framed stream.
use std::io::Write;
use std::time::Duration;

use anyhow::{Context, Result};
use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ToNsName};
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;

use seed_wire::{Action, Message, read_frame, write_frame};

const BRIDGE_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: u32 = 1;
const BACKOFFS_MS: &[u64] = &[200, 400, 800, 1600, 5000];

// ---------------------------------------------------------------------------
// Output frames (the bridge's line protocol)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OutFrame {
    Hello {
        bridge_version: &'static str,
        v: u32,
    },
    Health {
        connected: bool,
    },
    Snapshot {
        state: Value,
    },
    Diff {
        events: Value,
    },
}

fn emit(frame: &OutFrame) {
    let s = match serde_json::to_string(frame) {
        Ok(s) => s,
        Err(e) => {
            warn!("failed to serialise output frame: {e}");
            return;
        }
    };
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    if writeln!(lock, "{s}").is_err() {
        // stdout closed — downstream consumer hung up, exit cleanly.
        std::process::exit(0);
    }
    let _ = lock.flush();
}

// ---------------------------------------------------------------------------
// Socket helpers
// ---------------------------------------------------------------------------

/// Resolve the socket name serving this process's `SEED_HOME`, so the bridge
/// follows the same home as the daemon and TUI (default `~/.seed` unless the
/// env var says otherwise).
fn socket_name() -> Result<interprocess::local_socket::Name<'static>> {
    seed_wire::socket_name()
        .to_ns_name::<GenericNamespaced>()
        .context("build socket name")
}

// ---------------------------------------------------------------------------
// Run loop
// ---------------------------------------------------------------------------

/// One full lifecycle of: connect → Hello → Subscribe → forward frames.
/// Returns `Ok(())` on clean shutdown; `Err` on socket error (caller reconnects).
///
/// When `probe` is set, return `Ok(())` immediately after emitting the first
/// `Snapshot` — the one-shot mode used by `--probe` to verify the daemon link.
async fn run_once(quit_rx: &mut mpsc::Receiver<()>, probe: bool) -> Result<()> {
    let name = socket_name()?;
    use interprocess::local_socket::tokio::Stream;
    let conn = Stream::connect(name).await.context("connect to seedd")?;
    let (rd, mut wr) = tokio::io::split(conn);
    let mut rd = BufReader::new(rd);

    // Wait for Hello before subscribing — matches the TUI handshake.
    match read_frame(&mut rd).await? {
        Some(Message::Hello {
            daemon_version,
            protocol_version,
        }) => {
            info!(daemon_version, protocol_version, "daemon hello");
        }
        Some(other) => {
            warn!("expected Hello, got {other:?}");
        }
        None => anyhow::bail!("daemon closed before Hello"),
    }

    // Subscribe to receive Snapshot + StateDiff broadcasts.
    let sub = Message::Request {
        id: 1,
        action: Action::Subscribe,
    };
    write_frame(&mut wr, &sub).await?;

    // Connection established — signal health up.
    emit(&OutFrame::Health { connected: true });

    // Forward inbound frames until socket error or quit signal.
    loop {
        tokio::select! {
            frame = read_frame(&mut rd) => match frame {
                Ok(Some(Message::Snapshot { state })) => {
                    let v = serde_json::to_value(&*state).unwrap_or(Value::Null);
                    emit(&OutFrame::Snapshot { state: v });
                    if probe {
                        // One-shot: the handshake + first snapshot proves the
                        // link. Exit cleanly so callers can script around it.
                        return Ok(());
                    }
                }
                Ok(Some(Message::StateDiff { events })) => {
                    let v = serde_json::to_value(&events).unwrap_or(Value::Null);
                    emit(&OutFrame::Diff { events: v });
                }
                Ok(Some(Message::Response { .. })) => {
                    // The only Request we send is Subscribe(id=1); its Response
                    // is informational and not needed downstream.
                    debug!("received Response frame; ignoring");
                }
                Ok(Some(Message::Ping)) => {
                    let _ = write_frame(&mut wr, &Message::Pong).await;
                }
                Ok(Some(other)) => debug!("unhandled frame: {other:?}"),
                Ok(None) => {
                    debug!("daemon closed connection");
                    return Ok(());
                }
                Err(e) => anyhow::bail!("read error: {e}"),
            },
            Some(_) = quit_rx.recv() => {
                info!("quit received from stdin; exiting cleanly");
                return Ok(());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("SEED_LOG").unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "seed-bridge {BRIDGE_VERSION} — line-protocol adapter for seedd\n\n\
             USAGE:\n  \
             seed-bridge            Stream frames forever (hello/health/snapshot/diff),\n                         \
             reconnecting with backoff. The Quickshell consumer.\n  \
             seed-bridge --probe    One-shot: connect, emit hello + health + the first\n                         \
             snapshot, then exit 0. Exit 1 if the daemon can't be\n                         \
             reached. Use to verify the link without a Ctrl-C dance.\n  \
             seed-bridge --help     This help.\n\n\
             Reads `quit` on stdin for graceful shutdown. Logs to stderr (SEED_LOG)."
        );
        return Ok(());
    }
    let probe = args.iter().any(|a| a == "--probe");

    // Listen for `quit\n` on stdin → graceful shutdown.
    //
    // On EOF or read error we *don't* drop `quit_tx`: that would close the
    // channel, and `quit_rx.recv()` would resolve with `None` and trick the
    // run loop into thinking a quit was requested. Park forever instead so
    // the sender stays alive for the lifetime of the bridge.
    let (quit_tx, mut quit_rx) = mpsc::channel::<()>(1);
    tokio::spawn(async move {
        let stdin = tokio::io::stdin();
        let mut lines = BufReader::new(stdin).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if line.trim() == "quit" {
                        let _ = quit_tx.send(()).await;
                        return;
                    }
                }
                Ok(None) | Err(_) => {
                    std::future::pending::<()>().await;
                }
            }
        }
    });

    // Hello banner — emitted exactly once at startup so consumers can pin a
    // version before any state arrives.
    emit(&OutFrame::Hello {
        bridge_version: BRIDGE_VERSION,
        v: PROTOCOL_VERSION,
    });

    // --probe: a single bounded attempt. Emit health+snapshot and exit 0 on
    // success; on any error or a timeout, emit health:false and exit 1 so
    // scripts (and humans) get a clear "daemon reachable?" signal. No backoff
    // loop — probe must terminate.
    if probe {
        const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
        match tokio::time::timeout(PROBE_TIMEOUT, run_once(&mut quit_rx, true)).await {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(e)) => {
                debug!("probe failed: {e}");
                emit(&OutFrame::Health { connected: false });
                std::process::exit(1);
            }
            Err(_) => {
                warn!("probe timed out after {PROBE_TIMEOUT:?} with no snapshot");
                emit(&OutFrame::Health { connected: false });
                std::process::exit(1);
            }
        }
    }

    let mut attempt = 0usize;
    loop {
        match run_once(&mut quit_rx, false).await {
            Ok(_) => {
                // Clean shutdown (quit signal). Exit successfully.
                return Ok(());
            }
            Err(e) => {
                debug!("connection lost: {e}");
                emit(&OutFrame::Health { connected: false });
                if quit_rx.try_recv().is_ok() {
                    return Ok(());
                }
                let delay_ms = BACKOFFS_MS[attempt.min(BACKOFFS_MS.len() - 1)];
                attempt = attempt.saturating_add(1);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }
}
