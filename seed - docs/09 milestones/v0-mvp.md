---
date created: Tuesday, April 28th 2026, 12:00:00 pm
date modified: Wednesday, April 29th 2026, 7:53:24 am
cssclasses: []
tags:
  - milestone
  - v0
  - mvp
status: shipped
---

# V0 MVP

The locked initial-release plan, executed across six waves between 2026-04-22 and 2026-04-27. This milestone captures the historical record; active follow-up work lives in [[v0-1-0-punch-list]] and [[BACKLOG.kanban]].

## Locked Decisions (do not re-litigate)

| Decision | Choice |
|---|---|
| Stack | Rust + ratatui + crossterm |
| Layout | Cargo workspace: `seed-core` (lib), `seed-daemon` (bin), `seed-tui` (bin) |
| Reminder nag | Background daemon `seedd` + OS notifications via `notify-rust` |
| Sync shape | Append-only `events.jsonl` + derived snapshot. See [[event-sourcing]] and [[snapshot-and-replay]]. CRDT/iroh wraps later. |
| IPC | Local Unix socket / Windows named pipe via `interprocess`. Length-prefixed JSON. |
| Config | "Enough" defaults baked in + optional `~/.seed/config.toml` override. `seed init` scaffolds. |
| State dir | `~/.seed/` with `SEED_HOME` env override (mirrors pour's `POUR_HOME`) |
| Pour integration | Defer in v0. Keep `events.jsonl` schema versioned + namespaced + extensible so pour can tail later without coupling — see [[wire-versioning]]. |
| Mid-game start | Match prototype's `state.jsx::midJourneyState` (~total lvl 149, FROND/BLOOM tier). |

## Visual Ambition (the heart)

The mandala is the central reward surface. From the original chat:

- *"99 in all skills should be visually intense"*
- *"think bigger.. exponential"*
- *"weighting across each spectrum; even 99 should have some of the non max characters"*
- *"fractal influences go both micro and macro"*

Push ratatui hard:

- Braille (`⠀`–`⣿`) for sub-cell density in the inner core
- Block + half-block (`░▒▓█▀▄▌▐▖▗▘▙▚▛▜▝▞▟`) for outer rings/petals/aura
- Box-drawing for structural spokes
- Per-cell truecolor (24-bit, with 256-color fallback)
- Multi-trait layered hues (warm core / cool flow / accent rings / violet depth)
- Animation: per-frame char swap shimmer + hue rotation at zenith
- Deterministic from `(traits, seed)`

## Tasks

### TASK-001 · Bootstrap Workspace

- __Wave__: 1 · __Status__: shipped · __Notes__: [[build-log-01-workspace-bootstrap]]
- AC: `cargo build` succeeds at workspace root; each crate has stub `lib.rs` / `main.rs` that compiles; workspace `Cargo.toml` declares 3 members + shared `[workspace.dependencies]`; `seed-daemon` and `seed-tui` depend on `seed-core` via path; `.gitignore` covers `target/`, sandbox dirs, IDE crap; Rust 2024 edition.

### TASK-002 · Seed-core: Domain Types + Pure Logic

- __Wave__: 2 · __Status__: shipped · __Notes__: [[build-log-02a-seed-core-foundation]]
- AC: `Trait`, `Category`, `Reminder` types serde-derived; static `CATEGORIES` (9) and `REMINDERS` (20) tables matching `wellness/data.jsx` exactly; OSRS XP table built once via `OnceLock` matching JSX `buildXpTable`; `xp_for_level`, `level_for_xp`, `level_progress`, `xp_to_next`, `level_norm`, `xp_reward`, `xp_drain`; property tests (lvl 92 ≈ half of lvl 99; round-trip 1..=99); the [[reminder-lifecycle]] enum (`Off / Dormant / Due / Overdue`) with overdue at >1.5× interval; [[pure-core]] (no IO; clock injected); [[tier-progression]] table SEED..ZENITH with `tier_for(total_level)` lookup.

### TASK-003 · Seed-core: Events Module

- __Wave__: 2 · __Status__: shipped · __Notes__: [[build-log-02a-seed-core-foundation]]
- AC: `Event` enum covering reminder/trait/companion/config events, extensible via `Event::Unknown { kind, data }` fallback (the [[wire-versioning]] forward-compat hook); wire envelope `{ "v": 1, "ts": "…", "kind": "seed.<ns>.<event>", "data": {…} }`; all `kind` strings namespaced under `seed.*`; `apply_event(state, event) -> state` is the [[event-sourcing]] single fold; round-trip serde test for every variant; JSON shape locked by snapshot tests; documented in [[events-schema]].

### TASK-004 · Seed-core: Glyph Renderer (visual heart)

- __Wave__: 2 · __Status__: shipped · __Notes__: [[build-log-02b-glyph-renderer]] · __See also__: [[glyph-expansion]], [[glyph-layer-composition]]
- AC: `render_glyph(traits, seed, target) -> GlyphFrame` with the [[glyph-layer-composition]] structural layers (core / clarity rings / reach arms+branches+macro tendrils / spine / flow / space / depth / resonance / warmth / aura / halo); multi-tier palettes; per-cell `Color::Rgb`; symmetry mirroring; golden-file test (byte-equal); 159×79 frame in <16ms release; zenith only at all 9 traits ≥ 0.97; `apply_to_buf` writes directly into ratatui `Buffer`.

### TASK-005 · Seed-core: Config Module

- __Wave__: 2 · __Status__: shipped · __Notes__: [[build-log-02a-seed-core-foundation]]
- AC: `Config { active_hours, snooze_min, palette, reminders, notif_style }`; `Default` matches JSX `initialState` baseline; `load(seed_home)` reads `~/.seed/config.toml` if present, merges over defaults; `seed init` scaffolds annotated default; `SEED_HOME` env override; round-trip toml + tempfile-backed integration test.
- __Note__: `seed-core::config` retained file I/O at v0, breaking [[pure-core]] for this one module; restoring purity is tracked as TASK-018 in [[v0-1-0-punch-list]].

### TASK-006 · Seed-daemon: IPC Server + State Owner + Event Log Writer

- __Wave__: 3 · __Status__: shipped · __Notes__: [[build-log-03-seed-daemon]]
- AC: `seedd` owns canonical `State`; on startup performs [[snapshot-and-replay]] (loads `~/.seed/snapshot.json` then folds `events.jsonl` tail); periodic snapshot every N events or T seconds; accepts `interprocess` connections; length-prefixed JSON framing; `Request { id, action }` and `StateDiff { events }` message kinds; broadcasts every committed event; fsync after each event write; rejects malformed frames without panicking; single-instance lock; `--foreground` flag; structured logging via `tracing`; graceful Ctrl-C shutdown.

### TASK-007 · Seed-daemon: Scheduler + Notifications

- __Wave__: 3 · __Status__: shipped · __Notes__: [[build-log-03-seed-daemon]]
- AC: tokio task wakes every 30s, fires `notify-rust::Notification` on Dormant→Due transition; debounced via `last_notified_ms`; respects `active_hours`; respects per-reminder snooze; at most one notification per reminder per due window; XP drain on overdue (now folded into the same tick — see [[overdue-rollover]]); simulated-clock unit tests.

### TASK-008 · Seed-tui: IPC Client + State Mirror

- __Wave__: 4 · __Status__: shipped · __Notes__: [[build-log-04-seed-tui]]
- AC: `seed` connects to daemon socket on startup, auto-spawns `seedd` if absent; subscribes to `StateDiff`, applies events through `seed_core::apply_event` to maintain local mirror — TUI is a [[event-sourcing]] *reader*, never a writer; sends user actions as `Request`; auto-reconnect with exponential backoff (cap 5s); IPC on a separate tokio task with channel handoff to UI thread; panic hook + raw-mode guard.

### TASK-009 · Seed-tui: Full Ratatui Layout

- __Wave__: 4 · __Status__: shipped · __Notes__: [[build-log-04-seed-tui]]
- AC: orbit pane + side panel (LIST / LEVELS / LOG / CONFIG tabs) + bottom command bar; orbit slot picker ports JSX faithfully; truecolor with 256/16 fallback per [[terminal-capability-ladder]]; braille fallback (also per [[terminal-capability-ladder]]); 5 palettes (sage/dusk/mist/ember/moss); `--dev` / `SEED_DEV=1` appends a fifth TWEAKS tab; key bindings (`/`, `Enter`, `q`, `Tab`/`Shift+Tab`); command vocab covering all 20 verbs + `/<trait> <n>` debug + `help`; toast on completion; shimmer animation; clean resize.

### TASK-010 · Cross-cutting: Logging, Panics, Terminal Shims

- __Wave__: 4 · __Status__: shipped · __Notes__: [[build-log-04-seed-tui]]
- AC: `tracing` + `tracing-subscriber` in both binaries with `SEED_LOG=` override; daemon logs to `~/.seed/seedd.log` with daily rotation; TUI logs to `~/.seed/seed.log` (suppressed below WARN unless `SEED_LOG` set); panic hook in TUI restores terminal then re-panics; truecolor + braille probes documented in README.

### TASK-011 · README + Smoke Test + Docs

- __Wave__: 5 · __Status__: shipped · __Notes__: [[build-log-05-docs-and-smoke]]
- AC: `README.md` covers install, quickstart, config, event-schema pour-integration note, troubleshooting; [[events-schema]] documents every event variant with example JSON; build-log notes populated; `tests/smoke.rs` end-to-end test (spawn daemon to tempdir SEED_HOME, send Request, verify event written and broadcast).

### TASK-012 · Seed-tui: CONFIG Side-panel Tab + Config Round-trip

- __Wave__: 4 · __Status__: deferred to v0.2.0
- The full editable CONFIG tab with `toml_edit` round-trip and `ConfigChanged` reload was deferred. v0.1.0 ships a read-only cut as TASK-016 in [[v0-1-0-punch-list]]; the full editable surface returns in v0.2.0.

## XP Recalibration (Wave 6)

Wave 6 reshaped the XP economy to enforce a 1-year-to-99 contract uniformly across all 9 traits and pre-wired the two prestige systems' data models without UI surface. See:

- [[xp-pacing]] — rescaled OSRS curve, per-reminder XP rewards, pacing bands enforced by tests
- [[prestige-integrate]] — per-trait reset to 1 with persistent visual enhancement
- [[prestige-focus]] — `tokens` currency awarded every +99 cumulative levels gained
- [[build-log-06-xp-calibration]] — what shipped
- [[build-log-06b-redline-fixes]] — inspector findings + corrective patches

## Execution Waves (historical)

| Wave | Tasks | Notes |
|---|---|---|
| 1 | TASK-001 | [[build-log-01-workspace-bootstrap]] |
| 2 | TASK-002 + 003 + 005 | [[build-log-02a-seed-core-foundation]] |
| 2 | TASK-004 | [[build-log-02b-glyph-renderer]] |
| 3 | TASK-006 + 007 | [[build-log-03-seed-daemon]] |
| 4 | TASK-008 + 009 + 010 | [[build-log-04-seed-tui]] |
| 5 | TASK-011 | [[build-log-05-docs-and-smoke]] |
| 6 | XP calibration + prestige pre-wire | [[build-log-06-xp-calibration]], [[build-log-06b-redline-fixes]] |

Each wave was paired with a governor (planning) → architect (build) → inspector (audit) loop. The build-log notes record the architect output and inspector findings; this milestone is the consolidated narrative.

## See also

- [[v0-1-0-punch-list]] — active follow-up work (the v0.1.0 punch list).
- [[BACKLOG.kanban]] — active board.
- [[event-sourcing]], [[pure-core]], [[wire-versioning]], [[reminder-lifecycle]], [[tier-progression]], [[snapshot-and-replay]], [[glyph-layer-composition]], [[terminal-capability-ladder]] — the durable concepts this milestone leans on.
