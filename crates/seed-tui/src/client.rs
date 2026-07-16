/// IPC client: connects to seedd, auto-spawns if absent, handles reconnect.
///
/// Wire protocol types (`Message`, `Action`, `ResponseResult`) and the framing
/// helpers live in the [`seed_wire`] crate so seed-tui, seed-daemon, and
/// seed-bridge all agree on byte layout. Re-exported here under
/// `crate::client::{Message, Action, ResponseResult}` for backward compat with
/// existing call sites.
use anyhow::{Context, Result, bail};
use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ToNsName};
use std::path::Path;
use tokio::io::BufReader;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use tracing::{debug, info, warn};

pub use seed_wire::{Action, Message, ResponseResult, read_frame, write_frame};

// ---------------------------------------------------------------------------
// IpcClient
// ---------------------------------------------------------------------------

/// IPC client. Communicates with seedd via channel pairs.
/// All socket I/O runs on a dedicated tokio task.
pub struct IpcClient {
    /// Send outbound messages to daemon.
    pub outbound: mpsc::Sender<Message>,
    /// Receive inbound messages from daemon.
    pub inbound: mpsc::Receiver<Message>,
}

impl IpcClient {
    /// Connect to the daemon socket, or spawn `seedd` and wait up to 2s for it.
    /// Returns a connected `IpcClient` with a dedicated background I/O task.
    pub async fn connect_or_spawn(seed_home: &Path) -> Result<Self> {
        // Try connecting first.
        if let Ok(client) = Self::try_connect().await {
            return Ok(client);
        }

        // No daemon reachable — spawn and wait via the shared helper, then
        // establish the channel-based client. Retry the connect briefly: the
        // daemon answered Ping, but guard against a transient gap between the
        // readiness probe and this connect.
        ensure_daemon_ready(seed_home).await?;
        let mut last_err = None;
        for _ in 0..10 {
            match Self::try_connect().await {
                Ok(client) => return Ok(client),
                Err(e) => {
                    last_err = Some(e);
                    sleep(Duration::from_millis(200)).await;
                }
            }
        }
        Err(last_err.expect("loop runs at least once"))
    }

    async fn try_connect() -> Result<Self> {
        let name = socket_name()?;
        use interprocess::local_socket::tokio::Stream;
        let conn = Stream::connect(name)
            .await
            .context("connect to daemon socket")?;

        let (rd, wr) = tokio::io::split(conn);
        let rd = BufReader::new(rd);

        let (out_tx, out_rx) = mpsc::channel::<Message>(64);
        let (in_tx, in_rx) = mpsc::channel::<Message>(128);

        // Spawn the I/O task.
        tokio::spawn(ipc_io_task(rd, wr, out_rx, in_tx));

        Ok(IpcClient {
            outbound: out_tx,
            inbound: in_rx,
        })
    }

    /// Send a message to the daemon (non-blocking; drops if channel full).
    pub async fn send(&self, msg: Message) {
        if self.outbound.send(msg).await.is_err() {
            warn!("IpcClient: outbound channel closed");
        }
    }

    /// Receive the next inbound message, or None if the channel is closed.
    pub async fn recv(&mut self) -> Option<Message> {
        self.inbound.recv().await
    }
}

/// Background task: drives reads and writes on the socket, with automatic
/// reconnect on transient errors.
///
/// Backoff schedule: 200 ms → 400 → 800 → 1600 → cap 5000 ms.
/// After reconnect, re-sends `Action::Subscribe` to get a fresh snapshot.
async fn ipc_io_task<R, W>(
    mut rd: BufReader<R>,
    mut wr: W,
    mut out_rx: mpsc::Receiver<Message>,
    in_tx: mpsc::Sender<Message>,
) where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    // Inner I/O loop. Returns true if the outer channel was intentionally closed
    // (no reconnect needed), false if the connection dropped (attempt reconnect).
    let closed = run_io_loop(&mut rd, &mut wr, &mut out_rx, &in_tx).await;
    if closed {
        return;
    }

    // Connection dropped — enter reconnect loop.
    warn!("IPC disconnected — will retry with backoff");
    const BACKOFFS_MS: &[u64] = &[200, 400, 800, 1600, 5000];
    let mut attempt = 0usize;
    loop {
        let delay_ms = BACKOFFS_MS[attempt.min(BACKOFFS_MS.len() - 1)];
        warn!(
            "IPC reconnect attempt {} (delay {}ms)",
            attempt + 1,
            delay_ms
        );
        sleep(Duration::from_millis(delay_ms)).await;

        if in_tx.is_closed() {
            debug!("IPC: inbound consumer gone — stopping reconnect");
            return;
        }

        match socket_name() {
            Err(e) => {
                warn!("IPC: cannot build socket name: {e}");
            }
            Ok(name) => {
                use interprocess::local_socket::tokio::Stream;
                match Stream::connect(name).await {
                    Err(e) => {
                        debug!("IPC reconnect failed: {e}");
                    }
                    Ok(conn) => {
                        info!("IPC reconnected after {} attempt(s)", attempt + 1);
                        let (new_rd, new_wr) = tokio::io::split(conn);
                        let mut new_rd = BufReader::new(new_rd);
                        let mut new_wr = new_wr;

                        // Re-subscribe to receive a fresh snapshot.
                        let subscribe = Message::Request {
                            id: 1,
                            action: Action::Subscribe,
                        };
                        if let Err(e) = write_frame(&mut new_wr, &subscribe).await {
                            warn!("IPC: re-subscribe write failed: {e}");
                            // Try again next backoff cycle.
                            attempt += 1;
                            continue;
                        }

                        let closed =
                            run_io_loop(&mut new_rd, &mut new_wr, &mut out_rx, &in_tx).await;
                        if closed {
                            return;
                        }
                        // Dropped again — keep retrying but reset backoff.
                        warn!("IPC disconnected again — will retry");
                        attempt = 0;
                        continue;
                    }
                }
            }
        }
        attempt += 1;
    }
}

/// Runs the read/write select loop on an established connection.
/// Returns `true` when the outbound channel closes (intentional shutdown).
/// Returns `false` when the socket errors (reconnect should be attempted).
async fn run_io_loop<R, W>(
    rd: &mut BufReader<R>,
    wr: &mut W,
    out_rx: &mut mpsc::Receiver<Message>,
    in_tx: &mpsc::Sender<Message>,
) -> bool
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    loop {
        tokio::select! {
            // Outbound: UI → daemon
            msg = out_rx.recv() => {
                match msg {
                    Some(m) => {
                        if let Err(e) = write_frame(wr, &m).await {
                            warn!("IPC write error: {e}");
                            return false;
                        }
                    }
                    None => {
                        debug!("IPC outbound channel closed");
                        return true; // intentional shutdown
                    }
                }
            }
            // Inbound: daemon → UI
            frame = read_frame(rd) => {
                match frame {
                    Ok(Some(m)) => {
                        if in_tx.send(m).await.is_err() {
                            debug!("IPC inbound channel closed");
                            return true; // intentional shutdown
                        }
                    }
                    Ok(None) => {
                        debug!("IPC: daemon closed connection");
                        return false; // reconnect
                    }
                    Err(e) => {
                        warn!("IPC read error: {e}");
                        return false; // reconnect
                    }
                }
            }
        }
    }
}

/// Resolve the platform-appropriate socket name (mirrors seed-daemon::ipc::socket_name).
fn socket_name() -> Result<interprocess::local_socket::Name<'static>> {
    #[cfg(unix)]
    {
        "@seed-daemon.sock"
            .to_ns_name::<GenericNamespaced>()
            .context("failed to build socket name")
    }
    #[cfg(windows)]
    {
        let username = std::env::var("USERNAME").unwrap_or_else(|_| "seedd".to_string());
        let pipe = format!("seed-daemon-{username}");
        pipe.to_ns_name::<GenericNamespaced>()
            .context("failed to build pipe name")
    }
}

/// Probe the daemon socket with a single `Ping`, returning `true` if a live
/// daemon answers `Pong` within 1s. Mirrors `seed-daemon::ipc::probe_existing`.
async fn probe_ready() -> bool {
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
    if write_frame(&mut wr, &Message::Ping).await.is_err() {
        return false;
    }
    matches!(
        tokio::time::timeout(Duration::from_secs(1), read_frame(&mut rd)).await,
        Ok(Ok(Some(Message::Pong)))
    )
}

/// Ensure a daemon is reachable: probe the socket, and if none answers, spawn
/// `seedd` and poll (200ms increments, up to 2s) until it responds to `Ping`.
///
/// Shared by the TUI launch (`connect_or_spawn`) and the headless `seed log`
/// path so the probe→spawn→wait-for-ready logic lives in exactly one place.
/// Errors only if the spawn itself fails or the daemon never becomes ready.
pub async fn ensure_daemon_ready(seed_home: &Path) -> Result<()> {
    if probe_ready().await {
        return Ok(());
    }

    info!("no daemon reachable; spawning seedd");
    spawn_daemon(seed_home)?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        sleep(Duration::from_millis(200)).await;
        if probe_ready().await {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            bail!("daemon did not start within 2 seconds");
        }
    }
}

/// Send a single `Action` to the daemon and return its `ResponseResult`.
///
/// One request/response round-trip on a fresh connection: no `Subscribe`, no
/// `StateDiff` stream. Intended for headless one-shot subcommands (`seed log`,
/// and later `seed presence`) that mutate via an `Action` and read one reply.
/// The daemon remains the sole writer; this only sends the action and reads the
/// diff the daemon reports back.
pub async fn request_once(action: Action) -> Result<ResponseResult> {
    let name = socket_name()?;
    use interprocess::local_socket::tokio::Stream;
    let conn = Stream::connect(name)
        .await
        .context("connect to daemon socket")?;
    let (rd, mut wr) = tokio::io::split(conn);
    let mut rd = BufReader::new(rd);

    let request = Message::Request { id: 1, action };
    write_frame(&mut wr, &request).await?;

    // Read frames until our Response arrives (the daemon may send Hello first).
    loop {
        match read_frame(&mut rd).await? {
            Some(Message::Response { id: 1, result }) => return Ok(result),
            Some(_other) => continue,
            None => bail!("daemon closed connection before responding"),
        }
    }
}

/// Spawn `seedd` as a detached background process.
fn spawn_daemon(seed_home: &Path) -> Result<()> {
    use std::process::Command;

    // Locate the seedd binary alongside the current executable.
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("seed"));
    let seedd = exe
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(if cfg!(windows) { "seedd.exe" } else { "seedd" });

    let mut cmd = Command::new(&seedd);
    cmd.env("SEED_HOME", seed_home);

    // Windows: CREATE_NO_WINDOW | DETACHED_PROCESS so the daemon survives TUI exit.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }

    cmd.spawn()
        .with_context(|| format!("failed to spawn seedd at {}", seedd.display()))?;

    Ok(())
}
