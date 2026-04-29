/// `seedd` — seed wellness daemon.
///
/// Usage:
///   seedd [--foreground] [--version] [--help]
///
/// Without flags, runs detached with log rotation to `<seed_home>/seedd.log`.
/// `--foreground` logs to stderr instead.
use anyhow::Result;
use seed_core::seed_home;
use tracing_subscriber::EnvFilter;

use seed_daemon::daemon;
use seed_daemon::ipc;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let foreground = args.iter().any(|a| a == "--foreground");

    if args.iter().any(|a| a == "--version") {
        println!("seedd v{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help") {
        println!(
            "seedd v{}\nUsage: seedd [--foreground] [--version] [--help]",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }

    let seed_home = seed_home();

    // Initialise tracing.
    if foreground {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_env("SEED_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .with_writer(std::io::stderr)
            .init();
    } else {
        std::fs::create_dir_all(&seed_home).ok();
        let log_dir = seed_home.clone();
        let file_appender = tracing_appender::rolling::daily(log_dir, "seedd.log");
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_env("SEED_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .with_writer(non_blocking)
            .init();
        // Keep _guard alive for the process lifetime.
        std::mem::forget(_guard);
    }

    // Single-instance lock: probe for an existing daemon.
    if ipc::probe_existing().await {
        eprintln!("seedd: another daemon is already running. Exiting.");
        std::process::exit(1);
    }

    // Graceful Ctrl-C: send a Shutdown command into the daemon's command
    // channel so Daemon::shutdown() runs (final snapshot + log flush) before
    // the process exits. Do NOT call process::exit() here — let main() return
    // normally after the daemon loop completes (Fix 3).
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let mut shutdown_tx_opt = Some(shutdown_tx);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("received Ctrl-C — requesting graceful shutdown");
            // Signal the run_with_shutdown wrapper below.
            if let Some(tx) = shutdown_tx_opt.take() {
                let _ = tx.send(());
            }
        }
    });

    daemon::Daemon::run_with_shutdown(seed_home, foreground, shutdown_rx).await
}
