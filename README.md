# Seed

A terminal-native wellness companion that turns the small acts of taking care of yourself into a game you can see. Keyboard-first, offline-first, no account, no cloud. Built alongside [pour](https://pour.madigan.app/) — same ethos, different surface.

> __Status:__ pre-v0. `seed-core` (domain, events, levels, config) is landing; `seedd` and `seed` binaries are still stubs. The backlog in [`docs/backlog.md`](docs/backlog.md) is the canonical plan.

## Why

We don't drink enough water. We don't stand up. We forget to look out the window. Not because we don't want to — because the friction of remembering, tracking, and caring about it slowly burns out.

Seed is a companion that notices for you. A background daemon nudges at the right moments. A TUI shows a living glyph — a mandala that grows more intricate as you live better. Each reminder you answer pours XP into one of nine traits. At zenith (all traits ≥ 0.97) the glyph shimmers.

The care is the reward loop. The glyph is the proof.

```
seed              # open the companion TUI
seed init         # scaffold ~/.seed/ with an annotated config
seed water        # log a glass of water from the CLI
seed stretch      # log a stretch
seedd             # the daemon — spawns automatically if absent
```

## Install

> No prebuilt binaries yet — v0 is still in progress. For now, build from source.

**From source**:

```bash
git clone https://github.com/mads-jm/seed
cd seed
cargo build --release
# Binaries:
#   target/release/seed   (TUI)
#   target/release/seedd  (daemon)
```

Requires Rust 2024 edition (pinned via `rust-toolchain.toml`). No other system dependencies.

Linux desktop notifications require a running notification daemon (most DEs have one). macOS and Windows use their native notification center via `notify-rust`.

## Quick Start

**1. Initialize**

```bash
seed init
```

`seed init` scaffolds `~/.seed/` with an annotated `config.toml`. Run this once before first use. All Seed state lives under `~/.seed/` (override with `SEED_HOME`).

```
~/.seed/
  config.toml        # your overrides — everything is optional
  events.jsonl       # append-only event log (source of truth)
  snapshot.json      # periodic rollup for fast daemon startup
  seedd.sock         # Unix socket (Windows: \\.\pipe\seedd-<user>)
  seedd.log          # daemon log (rotated)
  seed.log           # TUI log (suppressed below WARN unless SEED_LOG=debug)
```

**2. Launch the companion**

```bash
seed
```

The TUI auto-spawns `seedd` in the background if it isn't already running, connects over a local socket, and subscribes to state updates. Close it with `q` — the daemon keeps running so you still get notifications.

**3. Log actions**

From inside the TUI, type into the command bar at the bottom:

```
water      # hydration
eat        # meal
stand      # get up
walk       # a walk
stretch    # stretch
look       # 20-20-20 eye break
breathe    # breathwork
rest       # screen break
align      # posture / alignment
```

Each action answers the currently-due reminder for that category, emits a `seed.reminder.completed` event, and pours XP into the relevant trait. A toast confirms the gain; the glyph responds.

Debug bump for a single trait: `/flow 50`.

## Config Overview

`~/.seed/config.toml` is optional — built-in defaults match the original prototype. Override only what you want to change:

```toml
config_version = "0.1.0"

# Don't nag outside these hours
active_hours = [7, 22]

# Default snooze duration (minutes) when you defer a reminder
snooze_min = 30

# Palette: sage | dusk | mist | ember | moss
palette = "sage"

# Notification style: silent | subtle | standard | alert
notif_style = "standard"

# Per-reminder overrides (anything missing falls through to the baked-in default)
[reminders.water]
interval_min = 45      # default 60
pinned = true          # stick this reminder into the orbit

[reminders.stretch]
enabled = false        # turn off entirely
```

### Reminders and categories

Nine categories, twenty reminders, baked into `seed-core`. Categories cover hydration, nourishment, movement, posture, vision, breath, rest, presence, and journaling. Each reminder has a baseline interval and an anchor hour; the daemon rolls them through `Dormant → Due → Overdue` and fires at most one OS notification per due window.

### State, events, replay

The daemon owns an in-memory `State` hydrated at startup from `snapshot.json` + a tail of `events.jsonl`. Every user action — complete, snooze, pin, enable, disable — becomes an event appended to the log (fsynced, one-per-line JSON with a versioned envelope):

```jsonl
{"v":1,"ts":"2026-04-22T09:12:03Z","kind":"seed.reminder.completed","data":{"id":"water"}}
{"v":1,"ts":"2026-04-22T09:12:03Z","kind":"seed.trait.xp_changed","data":{"trait":"flow","delta":12}}
```

The log is the source of truth. Delete `snapshot.json` at any time — the next daemon start will re-fold from events.

## Architecture

Three crates in one Cargo workspace:

| Crate         | Bin    | Role |
|---------------|--------|------|
| `seed-core`   | —      | Pure domain: categories, reminders, XP curve, tiers, events, state fold, glyph renderer, config parser. No I/O — the clock is an argument. |
| `seed-daemon` | `seedd` | Owns the canonical state, writes `events.jsonl`, schedules reminders, fires OS notifications, serves an IPC socket. |
| `seed-tui`    | `seed`  | Ratatui client. Connects to `seedd`, mirrors state via event diffs, renders the glyph + orbit + side panel, sends user actions back as requests. |

IPC is a local Unix socket (`~/.seed/seedd.sock`) or a Windows named pipe (`\\.\pipe\seedd-<user>`), length-prefixed JSON framing via `interprocess`. Single-instance lock on the daemon. Auto-reconnect with backoff on the client.

## The Glyph

The mandala is the heart of the surface. It renders into a ratatui `Buffer` at the native terminal resolution using layered character sets:

- **Braille** (`⠀`–`⣿`) for sub-cell density in the inner core
- **Block + half-block** (`░▒▓█▀▄▌▐▖▗▘▙▚▛▜▝▞▟`) for outer petals and aura rings
- **Box-drawing** for structural spokes
- **Per-cell truecolor** (24-bit RGB, with 256-color and 16-color fallbacks probed at startup)
- Multi-trait hue blending (warm core / cool flow / accent rings / violet depth)
- Deterministic from `(traits, seed)`; per-frame shimmer at zenith

Nine traits mapped onto nine structural layers. Level norm weights each layer's density. The glyph is the single visible consequence of your life — a reflection, not a score.

## Pour Integration

Seed and [pour](https://pour.madigan.app/) share a philosophy (write more... pour / care more... seed) and will share a data surface.

v0 defers integration but keeps the door open:

- `events.jsonl` uses a versioned envelope and `seed.*`-namespaced `kind` strings
- Pour can tail the log and write a daily-note row per `seed.reminder.completed` without either project needing to depend on the other
- `SEED_HOME` and `POUR_HOME` are siblings by convention

Full design: [`docs/events-schema.md`](docs/events-schema.md) (forthcoming).

## Tech Stack

| Area            | Crate |
|-----------------|-------|
| TUI             | `ratatui` + `crossterm` |
| Async runtime   | `tokio` |
| IPC             | `interprocess` (local socket / Windows named pipe) |
| OS notifications| `notify-rust` |
| Serialization   | `serde` + `serde_json` + `toml` + `toml_edit` |
| Time            | `chrono` |
| Logging         | `tracing` + `tracing-subscriber` + `tracing-appender` |
| Unicode         | `unicode-width` |
| Paths           | `dirs` |

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

Run the daemon in the foreground with verbose logging:

```bash
SEED_LOG=debug cargo run -p seed-daemon -- --foreground
```

Point a whole session at a throwaway state directory:

```bash
SEED_HOME=$(mktemp -d) cargo run -p seed-tui
```

Tests use `tempfile` for `SEED_HOME`, `pretty_assertions` for diffs, and golden-file snapshots for the glyph renderer. Keep them pure — the daemon's scheduler takes an injected clock.

## Documentation

- [`docs/backlog.md`](docs/backlog.md) — v0 MVP backlog (locked decisions, tasks, execution waves)
- [`docs/build-log/`](docs/build-log/) — per-milestone execution records (scope, shipped, deferrals)
- [`seed - docs/`](seed%20-%20docs/index.md) — Obsidian vault (concepts, architecture, ADRs, stories)

## License

MIT.
