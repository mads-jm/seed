# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`seed` is a local-first TUI wellness companion: a background daemon (`seedd`) fires OS notifications for nine wellness traits (flow, core, spine, reach, clarity, space, depth, resonance, warmth), and a ratatui terminal UI (`seed`) renders a generative mandala that grows with XP. State lives at `~/.seed/` (override with `SEED_HOME`). See `README.md` for product framing.

## Documentation

The canonical documentation lives in the Obsidian vault at `seed - docs/` (literal directory name, with the space-dash-space). The vault publishes to GitHub Pages via the Webpage HTML Export plugin — exported HTML is written to `docs/` at the project root, so `docs/` is build output, not a source tree. Don't author content under `docs/`; edit the vault.

Useful entry points inside the vault:

- `seed - docs/index.md` — root index, navigation hub.
- `seed - docs/00 index/BACKLOG.kanban.md` — active backlog (v0.1.0 punch list).
- `seed - docs/08 specs/v0-1-0-punch-list.md` — full prose for active backlog items.
- `seed - docs/09 milestones/v0-mvp.md` — historical record of the v0 MVP build (waves 1–5).
- `seed - docs/02 references/events-schema.md` — wire-protocol contract.
- `seed - docs/02 references/cli-flags.md` — CLI / env reference.
- `seed - docs/03 guides/cargo-cheatsheet.md` — cargo command reference.
- `seed - docs/05 notes/build-log-*.md` — per-wave execution records (transitive notes; folded into milestones over time).
- `seed - docs/08 specs/` — feature/component specs: `xp-pacing`, `prestige-integrate`, `prestige-focus`, `overdue-rollover`, `glyph-expansion`.

The vault has its own `CLAUDE.md` at `seed - docs/CLAUDE.md` documenting frontmatter, status conventions, and structural maintenance.

## Workspace layout

Cargo workspace, Rust 2024 edition (pinned via `rust-toolchain.toml`), three crates:

| Crate | Kind | Binary | Notes |
|---|---|---|---|
| `seed-core` | lib | — | Pure domain logic — no I/O. Clock/FS injected at call sites. |
| `seed-daemon` | bin + lib | `seedd` | Background daemon. Lib surface is for tests. |
| `seed-tui` | bin + lib | `seed` | TUI. Lib is `seed_tui_testlib` for integration tests. |

Versions are pinned at the workspace root (`Cargo.toml` `[workspace.dependencies]`) — add new deps there and reference with `crate.workspace = true`, don't pin per-crate.

## Common commands

`seed - docs/03 guides/cargo-cheatsheet.md` is the authoritative cheat sheet. Highlights:

```bash
cargo check --workspace                              # tightest feedback loop
cargo build --workspace                              # debug
cargo build --release --workspace                    # produces target/release/{seed,seedd}
cargo test --workspace                               # unit + integration
cargo test -p seed-core glyph -- --nocapture         # one crate, name-substring match
cargo test --test smoke -- --ignored                 # gated: spawns a real seedd
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

CI-equivalent local check:

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

There is **no `serve` binary** — the daemon is `seedd`.

### Running

The TUI auto-spawns `seedd` if no daemon is reachable, so `cargo run --bin seed` is enough for a normal run. To debug, run them separately:

```bash
# Terminal 1: verbose daemon, logs to stderr
SEED_LOG=debug cargo run --bin seedd -- --foreground
# Terminal 2: TUI
cargo run --bin seed
```

Sandbox a run away from real `~/.seed`:

```bash
SEED_HOME=/tmp/seed-sandbox cargo run --bin seed -- init
SEED_HOME=/tmp/seed-sandbox cargo run --bin seed
```

Useful env vars: `SEED_LOG` (tracing filter, e.g. `seed_daemon=debug`), `SEED_HOME` (override state dir), `SEED_DEV=1` (enables `--dev` TWEAKS tab), `SEED_FORCE_ASCII=1` / `SEED_FORCE_256=1` (terminal-compat fallbacks).

### Glyph golden snapshot

`crates/seed-core/tests/glyph_golden.txt` is byte-compared. After an intentional renderer change, regenerate then commit (LF-only, single trailing newline):

```bash
cargo test -p seed-core --test glyph dump_golden -- --ignored --nocapture
# copy lines between BEGIN/END SNAPSHOT markers into glyph_golden.txt
cargo test -p seed-core golden_snapshot
```

## Architecture

### Event-sourced core
`seed-core` is the source of truth for the domain model. State is the **fold of an append-only event log**:

- Events are defined in `seed-core/src/events.rs` as a tagged enum, serialized as `{ "v": 1, "ts": ..., "kind": "seed.<ns>.<event>", "data": {...} }`. Unknown kinds round-trip via `Event::Unknown` so downstream tools never lose data.
- `apply_event(state, event) -> state` is the single fold function; both daemon (writer) and TUI (reader, via StateDiff) use it.
- Wire schema is contract-frozen in `seed - docs/02 references/events-schema.md`. Versioning rules: adding a kind or a field is non-breaking; renaming/removing/retyping bumps `v`. Events with unknown major `v` are skipped, not crashed on.
- `seed-core` is **pure**: no filesystem, no clock. The clock comes in as `now: DateTime<Utc>` and paths come in as `&Path`. Keep it that way — it's why golden tests and replay work.

### Daemon
`seedd` (`crates/seed-daemon/`) owns the writable state and is the only writer:

- On boot: load `snapshot.json` (if any) → tail `events.jsonl` past `skip_count` → reach current state.
- Snapshots persist every 100 events or 5 minutes (`SNAPSHOT_EVENT_THRESHOLD` / `SNAPSHOT_TIME_SECS` in `daemon.rs`); ticks fire every 30s (`TICK_INTERVAL_SECS`).
- Single-instance lock: probes the IPC socket on startup; if a live daemon answers Ping, exits 1; if the socket is stale, removes it and proceeds.
- Graceful shutdown: Ctrl-C → oneshot → daemon loop drains, writes a final snapshot, flushes logs. **Don't `process::exit` from the signal handler** — let `main()` return after the loop.
- Notifications via `notify-rust`. The `Notifier` trait lets tests inject a fake.

### IPC
`tokio` over `interprocess` local sockets (Unix abstract `@seed-daemon.sock`, Windows named pipe `seed-daemon-<USERNAME>`). Framing: 4-byte big-endian length + UTF-8 JSON, max 4 MiB per frame. See `crates/seed-daemon/src/wire.rs`.

Top-level `Message` variants: `Hello`, `Request{id, action}`, `Response{id, result}`, `StateDiff{events}` (broadcast on commit), `Snapshot{state}` (sent once on `Subscribe`), `Ping`/`Pong`, `Error`. `Action` covers all mutations the TUI requests (Complete, Snooze, TogglePin, etc.).

Per-connection `mpsc` for responses (so a request from client A doesn't get routed to client B); `broadcast` for StateDiff fan-out.

### TUI
`crates/seed-tui/` is a thin client: subscribes to the daemon, mirrors state locally from Snapshot + StateDiff, never mutates directly — every change goes back through `Action`. Auto-reconnects with exponential backoff (200ms → 5s cap). Commands typed while disconnected are dropped; the status bar reflects this.

Render uses ratatui + crossterm. The mandala is the central widget — braille for inner core, block/half-block for petals/aura, box-drawing for spokes, per-cell truecolor with a 256-color quantization fallback.

## Conventions

- Format with rustfmt (Rust 2024 edition; `rustfmt.toml` is minimal). Lint clean: `clippy -D warnings`.
- Don't add new top-level deps without registering them in workspace `[workspace.dependencies]` first.
- Public surface area in `seed-daemon/src/lib.rs` and `seed-tui/src/lib.rs` exists for integration tests — keep things `pub` only when a test or the bin needs them; avoid leaking internals just because.
- When adding event variants: bump nothing (kinds are additive), add a serde round-trip test, document the kind in `seed - docs/02 references/events-schema.md`. When changing an existing variant's shape, that's a `v` bump and the schema doc must be updated in the same change.
- Sub-vault `seed - docs/` is an Obsidian vault with its own `CLAUDE.md` and conventions; don't apply Rust workspace rules there.
