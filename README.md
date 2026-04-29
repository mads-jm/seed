# seed

A local-first TUI wellness companion. Reminders you type. A mandala that grows with your care.

## Why

We don't drink enough water. We don't stand up. We forget to look out the window. Not because we don't want to — because the friction of remembering slowly burns out the impulse before we act on it.

Seed takes the energy of a progression game and turns it into something that cares about you. A background daemon tracks nine wellness traits — hydration, nourishment, movement, posture, vision, breath, rest, presence, alignment — and nudges at the right moments. In the terminal, a living mandala unfolds as you respond. Each reminder you answer pours XP into a trait. The glyph reflects that back.

The companion should grow, mature, change, evolve endlessly based on your success. At level 1 it's a sparse seed. At level 99 across all nine traits it shimmers at zenith. The care is the reward loop. The glyph is the proof.

## What you get

- Background daemon (`seedd`) that tracks reminders and fires OS notifications
- Terminal UI (`seed`) with a generative mandala that evolves with 9 wellness traits: flow, core, spine, reach, clarity, space, depth, resonance, warmth
- Type the action word to log a reminder. mandala grows. levels rise.
- Local-first. No cloud. State at `~/.seed/`. Event-log shaped so a future p2p sync layer can wrap without a rewrite.
- Cosmetic palettes (sage / dusk / mist / ember / moss), OSRS-style XP progression (1-99, lvl 92 ≈ halfway), SEED → ZENITH tier progression

## Install

From source (only install method at v0):

```
git clone <this repo>
cd seed
cargo build --release
```

Binaries land in `target/release/`:
- `seed` — the TUI
- `seedd` — the daemon (auto-spawned by `seed` if not running)

Requires Rust 2024 edition (pinned via `rust-toolchain.toml`). No other system dependencies.

Linux desktop notifications require a running notification daemon (most DEs ship one). macOS and Windows use their native notification center via `notify-rust`.

## Quickstart

```
seed init     # scaffold ~/.seed/config.toml
seed          # launch the TUI (auto-spawns seedd in the background)
```

First run: the mandala appears as a sparse seed. Type `water` + Enter. Watch the flow trait XP tick up. Do the same across the 9 traits over time and the mandala unfolds.

## Commands

- `<verb> <Enter>` — log a reminder: `water`, `eat`, `stand`, `walk`, `stretch`, `look`, `breathe`, `rest`, `align`
- `/<trait> <level>` — debug: set a trait to any level 1-99 (e.g. `/flow 50`)
- `help` — command reference
- `/` — focus the command bar
- `Tab` / `Shift+Tab` — cycle side panel tabs (LIST / LEVELS / LOG)
- `q` — quit

### Dev mode

Launch with `--dev` (or `SEED_DEV=1`) to expose an extra side-panel tab:

```
seed --dev
```

The **TWEAKS** tab adds a palette selector, manual reminder trigger, and state reset — useful while iterating, but gated behind a flag so everyday use stays driven by reminders alone. There is no default hotkey for this tab; it's reached via `Tab` like the others once enabled.

## Config

Everything lives at `~/.seed/` (override with `SEED_HOME`):

```
~/.seed/
  config.toml       # edit me
  events.jsonl      # append-only reminder log
  snapshot.json     # periodic state snapshot
  seedd.log         # daemon logs (daily-rotated)
  seed.log          # TUI logs
```

`config.toml` example (copy from the scaffold — run `seed init` first):

```toml
# seed configuration
# All keys are optional — omit any line to keep the default.

# Hours during which notifications are active (24-hour clock).
active_hours = [7, 22]

# Default snooze duration in minutes.
snooze_min = 5

# Colour palette: sage | dusk | mist | ember | moss
palette = "sage"

# Notification style: standard | flash | silent
notif_style = "flash"

# Deterministic seed for glyph generation.
glyph_seed = 42

# Per-reminder overrides. Omit any section to use the default.
# [reminders.water]
# interval_min = 45
# enabled = true
```

Key fields:
- `active_hours = [7, 22]` — notifications fire only between 7am and 10pm
- `snooze_min = 5` — default snooze duration
- `palette = "sage"` — one of sage / dusk / mist / ember / moss
- `[reminders.water]` — per-reminder override of `interval_min` and `enabled`

## Terminal compatibility

seed renders with braille + truecolor by default. On terminals that don't support them:

- Braille: set `SEED_FORCE_ASCII=1` to swap the glyph's inner core to block characters
- Truecolor: set `SEED_FORCE_256=1` to quantize to the 256-color cube

Tested on: Windows Terminal, iTerm2, kitty, wezterm, alacritty.

## Troubleshooting

**Stopping the daemon on Windows.** `seed` auto-spawns `seedd` as a detached process. Closing the TUI does not stop it (by design — you still get notifications). To stop it manually: `taskkill /IM seedd.exe /F` in a shell, or end it from Task Manager.

**Glyph appears empty on cold start.** `seed` connects to the daemon and waits up to 3 seconds for the initial state snapshot. On slow disks or with antivirus scanning, the daemon may take a moment to start. The glyph will fill in when the snapshot arrives; any commands typed in the interim are safe.

**No notifications despite being in active hours.** Check that `active_hours` in `config.toml` is set correctly (e.g. `active_hours = [7, 22]`). A zero-length window like `[7, 7]` silently disables all notifications — the config loader rejects this with a clear error.

**Daemon disconnects during a session.** The TUI auto-reconnects with exponential backoff (200ms → 5s cap). Commands sent while disconnected are lost — the status bar will show reconnecting. If the glyph freezes and commands have no effect, check `~/.seed/seedd.log`.

## Pour integration

`seed` is a sibling to [pour](https://github.com/mads-jm/pour) — a terminal capture tool. A future pour module will tail `~/.seed/events.jsonl` to pour structured reminder entries into your Obsidian vault. v0 exposes the event schema (see `seed - docs/02 references/events-schema.md`) but ships no coupling. Either project works standalone.

## Tech stack

| Area          | Crate                                      |
|---------------|--------------------------------------------|
| TUI           | `ratatui` + `crossterm`                    |
| IPC           | `interprocess` (local socket / named pipe) + length-prefixed JSON |
| Async         | `tokio`                                    |
| Serialization | `serde` + `serde_json` + `toml`            |
| Notifications | `notify-rust`                              |
| Time          | `chrono`                                   |
| Logging       | `tracing` + `tracing-appender`             |

## Development

```
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all
```

Run the daemon in the foreground with verbose logging:

```
SEED_LOG=debug cargo run --bin seedd -- --foreground
```

Run the smoke test (spawns a real daemon process):

```
cargo test --test smoke -- --ignored
```

Full docs in the Obsidian vault at `seed - docs/`. Wave-by-wave build logs in `seed - docs/05 notes/build-log-*.md`; consolidated v0 plan in `seed - docs/09 milestones/v0-mvp.md`; active backlog in `seed - docs/00 index/BACKLOG.kanban.md` (board) and `seed - docs/08 specs/v0-1-0-punch-list.md` (prose). The `docs/` directory at the project root is the GitHub Pages export target — build output, not source.

## License

MIT.
