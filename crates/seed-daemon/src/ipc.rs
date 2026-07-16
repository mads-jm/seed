/// IPC server: accepts local socket connections, routes frames to daemon.
///
/// Single-instance lock: attempts a client ping before binding. If a live
/// daemon responds, this process exits early. If the socket path is stale
/// (no response), removes the stale socket and proceeds.
///
/// Each connection runs in its own task. Subscribed connections receive
/// `StateDiff` pushes via a `broadcast::Receiver`. Responses are routed
/// directly to the originating connection via a per-connection `mpsc` channel.
use anyhow::{Context, Result};
use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Name, ToNsName};
use seed_core::EventEnvelope;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, warn};

use crate::wire::{Action, Message, read_frame, write_frame};

/// Unique identifier for a connected client.
pub type ConnId = u64;

/// Commands sent from IPC connection tasks back to the Daemon main loop.
#[derive(Debug)]
pub enum Command {
    /// An action received from a client. `resp_tx` is the per-connection
    /// sender used to route the response back to this specific client only.
    Action {
        conn_id: ConnId,
        request_id: u64,
        action: Action,
        /// One-shot response channel back to this specific connection.
        resp_tx: mpsc::Sender<Message>,
    },
    /// Connection is closing; daemon should clean up its resp_tx entry.
    Disconnect { conn_id: ConnId },
    #[allow(dead_code)]
    Shutdown,
}

/// Resolve the socket name serving this process's `SEED_HOME`.
///
/// The name itself comes from `seed_wire::socket_name()` so daemon, TUI, and
/// bridge derive it identically; this only builds the `interprocess` `Name`.
pub fn socket_name() -> Result<Name<'static>> {
    seed_wire::socket_name()
        .to_ns_name::<GenericNamespaced>()
        .context("failed to build socket name")
}

/// Attempt a single Ping to an existing daemon. Returns `true` if a live
/// daemon is already running.
pub async fn probe_existing() -> bool {
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
    let mut rd = tokio::io::BufReader::new(rd);
    if write_frame(&mut wr, &Message::Ping).await.is_err() {
        return false;
    }
    matches!(
        tokio::time::timeout(std::time::Duration::from_secs(1), read_frame(&mut rd)).await,
        Ok(Ok(Some(Message::Pong)))
    )
}

/// Run the IPC listener. Accepts connections and spawns a handler task for each.
///
/// `cmd_tx` routes `Command::Action` and `Command::Shutdown` to the daemon loop.
/// `diff_tx` is a broadcast sender; each subscribed connection gets a receiver.
pub async fn run_listener(
    cmd_tx: mpsc::Sender<Command>,
    diff_tx: broadcast::Sender<Vec<EventEnvelope>>,
) -> Result<()> {
    let name = socket_name()?;
    let opts = ListenerOptions::new().name(name);
    let listener = opts.create_tokio().context("failed to bind IPC listener")?;
    info!("IPC listener ready");

    let mut next_conn_id: ConnId = 1;

    loop {
        match listener.accept().await {
            Ok(conn) => {
                let conn_id = next_conn_id;
                next_conn_id = next_conn_id.wrapping_add(1);
                let cmd_tx2 = cmd_tx.clone();
                let diff_rx = diff_tx.subscribe();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(conn, conn_id, cmd_tx2, diff_rx).await {
                        debug!("IPC connection {conn_id} closed: {e}");
                    }
                });
            }
            Err(e) => {
                warn!("IPC accept error: {e}");
            }
        }
    }
}

/// Handle a single IPC connection. Reads requests, writes responses/diffs.
///
/// Two write paths share the connection's write half:
/// 1. `resp_rx` — per-connection `mpsc` for responses targeted at this client.
/// 2. `diff_rx` — broadcast receiver for `StateDiff` events pushed to all.
async fn handle_connection(
    stream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + 'static,
    conn_id: ConnId,
    cmd_tx: mpsc::Sender<Command>,
    mut diff_rx: broadcast::Receiver<Vec<EventEnvelope>>,
) -> Result<()> {
    let (rd, wr) = tokio::io::split(stream);
    let mut rd = tokio::io::BufReader::new(rd);
    // Wrap write half in an Arc<Mutex> shared between the response writer and
    // the diff-push task.
    let wr = std::sync::Arc::new(tokio::sync::Mutex::new(wr));

    // Per-connection response channel: daemon sends Message::Response here;
    // the write task below forwards it to the wire.
    let (resp_tx, mut resp_rx) = mpsc::channel::<Message>(64);

    // Send Hello.
    {
        let mut w = wr.lock().await;
        write_frame(
            &mut *w,
            &Message::Hello {
                daemon_version: env!("CARGO_PKG_VERSION").to_string(),
                protocol_version: 1,
            },
        )
        .await?;
    }

    // Spawn write task: merges per-connection responses and broadcast diffs.
    let wr_write = wr.clone();
    let write_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                // Response targeted at this connection.
                msg = resp_rx.recv() => {
                    match msg {
                        Some(m) => {
                            let mut w = wr_write.lock().await;
                            let _ = write_frame(&mut *w, &m).await;
                        }
                        None => break, // sender dropped (connection closing)
                    }
                }
                // Broadcast diff for all subscribers.
                diff = diff_rx.recv() => {
                    match diff {
                        Ok(envelopes) => {
                            let mut w = wr_write.lock().await;
                            let _ = write_frame(&mut *w, &Message::StateDiff { events: envelopes }).await;
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("IPC subscriber {conn_id} lagged {n} messages");
                        }
                    }
                }
            }
        }
    });

    // Main read loop.
    loop {
        match read_frame(&mut rd).await {
            Ok(Some(msg)) => match msg {
                Message::Ping => {
                    let mut w = wr.lock().await;
                    write_frame(&mut *w, &Message::Pong).await?;
                }
                Message::Request { id, action } => {
                    let shutdown = matches!(action, Action::Shutdown);
                    cmd_tx
                        .send(Command::Action {
                            conn_id,
                            request_id: id,
                            action,
                            resp_tx: resp_tx.clone(),
                        })
                        .await
                        .context("daemon command channel closed")?;
                    if shutdown {
                        break;
                    }
                }
                _ => {
                    debug!("unexpected message from client {conn_id}: ignored");
                }
            },
            Ok(None) => break, // clean EOF
            Err(e) => {
                warn!("IPC frame error on connection {conn_id}: {e}");
                break;
            }
        }
    }

    // Notify daemon to clean up this connection's response sender.
    let _ = cmd_tx.send(Command::Disconnect { conn_id }).await;

    write_handle.abort();
    Ok(())
}
