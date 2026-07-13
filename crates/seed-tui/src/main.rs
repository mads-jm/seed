/// `seed` — seed wellness TUI.
///
/// Usage:
///   seed [init] [--seed-home <path>] [--version] [--help]
///
/// Without subcommand, launches the full TUI.
/// `seed init` scaffolds `~/.seed/config.toml`.
use anyhow::Result;
use seed_core::seed_home as default_seed_home;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

mod app;
mod client;
mod command;
mod init;
mod input;
mod logcmd;
mod palette;
mod prestige;
mod term;
mod view;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // ── --version / -V ───────────────────────────────────────────────────────
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("seed v{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // ── --help / -h ──────────────────────────────────────────────────────────
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }

    // ── --seed-home / $SEED_HOME ─────────────────────────────────────────────
    let seed_home: PathBuf = if let Some(pos) = args.iter().position(|a| a == "--seed-home") {
        args.get(pos + 1)
            .map(PathBuf::from)
            .unwrap_or_else(default_seed_home)
    } else {
        default_seed_home()
    };

    // ── Tracing (file rotation) ──────────────────────────────────────────────
    std::fs::create_dir_all(&seed_home).ok();
    let file_appender = tracing_appender::rolling::daily(&seed_home, "seed.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("SEED_LOG").unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();
    // Keep guard alive for process lifetime.
    std::mem::forget(_guard);

    // ── Panic hook: restore terminal before re-panicking ─────────────────────
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Best-effort terminal restore.
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::cursor::Show,
            crossterm::terminal::LeaveAlternateScreen,
        );
        let _ = std::io::Write::flush(&mut std::io::stdout());
        default_hook(info);
    }));

    // ── Subcommands ──────────────────────────────────────────────────────────
    // Each headless subcommand handles its own connect/spawn and exits without
    // launching the TUI. The seam here (match on args[1]) is where a sibling
    // `presence` subcommand will slot in, reusing client::request_once.
    match args.get(1).map(|s| s.as_str()) {
        Some("init") => return init::run_init(&seed_home),
        Some("log") => {
            let code = logcmd::run(&seed_home, &args).await?;
            std::process::exit(code);
        }
        _ => {}
    }

    // ── Full TUI ─────────────────────────────────────────────────────────────
    app::App::run(seed_home).await
}

fn print_help() {
    println!(
        r#"seed v{}
Wellness companion TUI

USAGE:
  seed              Launch the TUI (connects to seedd daemon)
  seed init         Scaffold ~/.seed/config.toml
  seed log <verb>   Log a completion headlessly (no TUI); auto-spawns seedd.
                    Add --json for a machine-readable XP/level diff.
  seed --version    Print version
  seed --help       Print this help

OPTIONS:
  --seed-home <path>   Override seed home directory (also: $SEED_HOME)

KEYS:
  /         Focus command bar
  ENTER     Submit command (or activate selected row when bar is empty)
  TAB       Cycle side panel tab
  ↑ / ↓     Move selection in side panel
  PGUP/PGDN Page through side panel
  SPACE     Pin/unpin selected row (when bar is empty)
  CTRL+E    Toggle enabled on selected reminder
  CTRL+T    Toggle tweaks panel
  CTRL+C    Quit

COMMANDS:
  water, steep, eat, graze, stand, align, walk, stretch,
  shake, look, sun, breathe, rest, journal, reflect,
  thanks, sit, read, tidy, reach

  /trait n  Debug: set trait to level n (e.g. /flow 50)
  help      Show command reference
"#,
        env!("CARGO_PKG_VERSION")
    );
}
