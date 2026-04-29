---
date created: Monday, April 27th 2026, 9:00:00 am
date modified: Wednesday, April 29th 2026, 7:53:27 am
cssclasses: []
tags:
  - reference
  - cli
  - flags
status: implemented
---

# CLI Flags & Environment Variables

Reference for `seed` (TUI), `seedd` (daemon), and the environment variables that influence both.

## `seed` — The TUI

```
seed                   # launch the TUI; auto-spawns seedd if not running
seed init              # scaffold ~/.seed/config.toml from defaults
seed --version | -V    # print version
seed --help    | -h    # print help
```

| Flag | Effect |
|------|--------|
| `--seed-home <path>` | Override the seed home directory for this process. Equivalent to `$SEED_HOME`. The flag wins over the env var when both are set. |

### Subcommands

| Command | Effect |
|---------|--------|
| `init` | Writes an annotated `config.toml` to `<seed_home>/config.toml`. Idempotent; existing files are not overwritten — if a config already exists, init reports the path and exits without writing. |

### TUI Key Bindings

| Key | Action |
|-----|--------|
| `/` | Focus command bar |
| `Enter` | Submit command (or activate selected row when the bar is empty) |
| `Tab` / `Shift+Tab` | Cycle side-panel tabs |
| `↑` / `↓` | Move selection in side panel |
| `PgUp` / `PgDn` | Page through side panel |
| `Space` | Pin / unpin selected reminder (when bar is empty) |
| `Ctrl+E` | Toggle enabled on the selected reminder |
| `Ctrl+T` | Toggle the TWEAKS panel. (Currently always reachable; the `--dev` / `SEED_DEV` gate is tracked as TASK-015 in [[v0-1-0-punch-list]] and not yet wired.) |
| `Ctrl+C` | Quit |

### TUI Command Vocab

The 20 reminder verbs (one per reminder in the catalog), grouped by trait:

| Trait | Verbs |
|---|---|
| flow | `water`, `steep` |
| core | `eat`, `graze` |
| spine | `stand`, `align` |
| reach | `walk`, `stretch`, `shake` |
| clarity | `look`, `sun` |
| space | `breathe`, `rest` |
| depth | `journal`, `reflect`, `thanks` |
| resonance | `sit`, `read` |
| warmth | `tidy`, `reach` |

The `reach` verb is reused — it is both the trait id and a reminder word under `warmth` (reach out to a friend). The parser routes against `REMINDERS.word`; trait lookups via `/<trait>` use the trait id.

Plus help and debug commands:

| Command | Effect |
|---------|--------|
| `help` or `?` | Show inline command reference. |
| `?<skill>` | Open the skill detail panel for the named trait (e.g. `?flow`, `?warmth`). Unknown skill names produce a "no such skill" inline error rather than opening the detail. |
| `/<trait> <n>` | Debug: set the trait's XP to the value at level `n` (1–99 clamped). e.g. `/flow 50`. |
| `/all <n>` | Debug: set all 9 traits to level `n` (1–99 clamped). |
| `/random` | Debug: randomise all 9 trait levels in 1–99. Takes no args; `/random foo` is rejected. |

## `seedd` — The Daemon

```
seedd                  # run detached; logs to <seed_home>/seedd.log with daily rotation
seedd --foreground     # run attached; logs to stderr
seedd --version
seedd --help
```

| Flag | Effect |
|------|--------|
| `--foreground` | Stay in the foreground. Logs go to stderr instead of the rotating file. Used for debugging and CI. |

The daemon refuses to start if another instance is already responding on its IPC socket (single-instance lock). It traps `Ctrl+C` for graceful shutdown — flushes the final snapshot and log before exiting.

## Environment Variables

| Variable | Used by | Effect | Default |
|----------|---------|--------|---------|
| `SEED_HOME` | both | Override the state directory. Mirrors `POUR_HOME` from the sibling `pour` project. | `~/.seed/` |
| `SEED_LOG` | both | `tracing_subscriber::EnvFilter` directive (e.g. `debug`, `seed_daemon=trace`, `info`). | TUI: `warn`; daemon: `info` |
| `SEED_DEV` | TUI | Documented as "enables the dev-mode TWEAKS panel and dev-only commands" but __not yet read by the TUI__. Tracked as TASK-015 in [[v0-1-0-punch-list]]. | unset |
| `SEED_FORCE_ASCII` | TUI | When set, the glyph renderer falls back to block characters instead of braille for the inner core. Use on terminals that don't render braille. See [[terminal-capability-ladder]]. | unset |
| `SEED_FORCE_256` | TUI | When set, the glyph renderer quantizes truecolor (24-bit) to the 256-color cube. Use on terminals without truecolor. See [[terminal-capability-ladder]]. | unset |

### Notes

- `SEED_LOG` accepts the full `EnvFilter` syntax: `SEED_LOG="seed_daemon=debug,seed_core=trace"` is valid and only raises log level for those crates.
- `SEED_FORCE_ASCII` and `SEED_FORCE_256` are diagnostic overrides — the renderer auto-probes at startup and only swaps in fallbacks where the terminal lacks support. The env vars force the fallback even on capable terminals (useful for screenshots and tests).
- `SEED_HOME` accepts an absolute or relative path. Relative paths are resolved against the process's working directory at startup.

## Common Invocations

```bash
# verbose daemon for development
SEED_LOG=debug cargo run -p seed-daemon -- --foreground

# verbose TUI
SEED_LOG=debug cargo run -p seed-tui

# run TUI against an isolated seed home (for testing)
SEED_HOME=/tmp/seed-test cargo run -p seed-tui

# enable dev mode for the TUI (TWEAKS tab)
SEED_DEV=1 cargo run -p seed-tui

# force ASCII fallback (e.g. for terminal screenshots)
SEED_FORCE_ASCII=1 cargo run -p seed-tui

# stop a detached daemon on Windows
taskkill /IM seedd.exe /F
```

## See also

- [[xp-pacing]] — XP curve and per-reminder rewards
- [[prestige-integrate]] — per-trait reset prestige (visual)
- [[prestige-focus]] — token-driven XP multiplier prestige
- [[reminder-lifecycle]] — the state machine the verbs drive transitions through
- [[terminal-capability-ladder]] — what `SEED_FORCE_*` variables override
- [[ARCHITECTURE]] — workspace layout and crate boundaries
