---
date created: Tuesday, April 28th 2026, 12:00:00 pm
date modified: Wednesday, April 29th 2026, 7:53:23 am
cssclasses: []
tags:
  - note
  - build-log
  - wave-4
status: archived
---

# Build Log 04 — Seed-tui

__Tasks__: TASK-008 · TASK-009 · TASK-010 · __Wave__: 4

## Scope

The TUI: IPC client + state mirror (TASK-008), full ratatui layout (TASK-009), cross-cutting logging/panic/terminal shims (TASK-010). The TUI is a thin reader — it folds incoming `StateDiff` events through `seed_core::apply_event` to mirror state locally and never mutates ([[event-sourcing]]). The render path implements the [[terminal-capability-ladder]] (truecolor → 256 → ASCII; braille → block). Initial pass: 204/204 tests green; after Wave 4.1 fixes, 220/220.

## What Shipped

| File | Purpose |
|---|---|
| `main.rs` | CLI dispatch (TUI / `init` / `--version` / `--help`), tracing init, panic hook |
| `app.rs` | `App` struct, event loop with `tokio::select!` over input/IPC/tick, render dispatch |
| `client.rs` | `IpcClient`: wire types (duplicated from `seed-daemon`), framing, `connect_or_spawn`, detached spawn, I/O task |
| `command.rs` | `ParsedCommand` enum + `parse()` covering all 20 reminder verbs and trait debug commands |
| `input.rs` | crossterm `Event` → `Action` mapping |
| `init.rs` | `seed init` subcommand wrapping `seed_core::config::scaffold_default` |
| `palette.rs` | 5 palettes (sage / dusk / mist / ember / moss); `palette_for()`; `downgrade_color()` with 256-cube + grayscale-ramp quantization |
| `term.rs` | `TerminalGuard` RAII + `truecolor_supported()` + `braille_supported()` env probes |
| `view/*.rs` | `orbit` (glyph + 8 orbital cards + shimmer), `side_panel` (LIST/LEVELS/LOG tabs), `title_bar`, `command_bar`, `status_bar`, `toast`, `tweaks` (Ctrl+T) |

## Technical Decisions

- __Wire types duplicated in `client.rs`__ — at v0, `seed-daemon` had no `[lib]` target; binary crates can't be imported. ~100 lines of pure serde structs duplicated rather than restructure the daemon for a couple of types. JSON compatibility is the contract. (Wave 5 added a daemon `lib.rs`; deduplicating is now TASK-020 in [[v0-1-0-punch-list]].)
- __Terminal capabilities are env-driven, not detected__ — `$COLORTERM in ("truecolor", "24bit")` for truecolor; `SEED_FORCE_256` and `SEED_FORCE_ASCII` as escape hatches. Querying terminal emulators for Unicode coverage is infeasible; documented in `--help`. See [[terminal-capability-ladder]].
- __Braille fallback is a TUI-side post-process__ — rather than a renderer overload in `seed-core`, `braille_to_block()` maps each braille char to a block-density equivalent based on popcount. Keeps the [[pure-core]] invariant intact.
- __Daemon auto-spawn on Windows uses `DETACHED_PROCESS | CREATE_NO_WINDOW`__ — without these flags the daemon shares the TUI's console and dies on `CTRL_CLOSE_EVENT` when the TUI exits. The daemon binary is located via `current_exe().parent()`.
- __Shimmer is deterministic__ — hash of `(position, tick/3)` at ~6 Hz, gate 1/31 cells at intensity ≥ 4. Same `(position, tick)` always swaps the same way; no flicker, no double-buffer artifacts.
- __`IpcClient` uses a separate tokio task + channel handoff__ — all socket I/O in `ipc_io_task`, `mpsc::Receiver<Message>` to UI. The render loop's `select!` reads from `client.inbound` and never blocks on socket I/O.
- __Panic hook restores terminal before invoking the default hook__ — `disable_raw_mode()` + `LeaveAlternateScreen` + cursor show happen first, then the captured default hook runs. `TerminalGuard::Drop` provides a second restore path for normal exits. Both calls to `disable_raw_mode()` are idempotent.

## Inspector Findings & Fixes (Wave 4.1)

The inspector pass cleared the critical path (gates green, happy path works) but flagged seven issues worth fixing before smoke-testing.

| Finding | Fix |
|---|---|
| __F1__ · Build-log claimed "Palette selection via tweaks sends SetPalette." Reality: `grep SetPalette` showed exactly one hit — the enum definition. The tweaks panel was a `Widget::render` impl with no event handling. The panel was 100% display-only; the build-log lied. | Replaced with `TweaksPanelState` + `TweakAction` event model. `handle_key → Option<TweakAction>` (SetPalette / TriggerReminderNow / Reset). RESET requires two-key confirm (Enter → `y`). New `Action::Reset` added to wire + daemon (replaces state with `initial_state()`, emits `CompanionAwakened`, sends fresh Snapshot). 6 new unit tests. |
| __F4__ · `&card.name[..card.name.len().min(7)]` — byte-index slice on `&'static str`. ASCII-only catalog made it safe today; one non-ASCII reminder name = char-boundary panic mid-render-loop, mid-raw-mode. | Replaced with `card.name.chars().take(7).collect::<String>()`. Audited every `view/*.rs` for other byte-index slices on `&str` (none). 2 new tests covering ASCII + multibyte. |
| __F2__ · Auto-reconnect (TASK-008 AC) was unimplemented. On `None` from `client.recv()` the TUI just logged `warn!` — no retry, no backoff, no re-subscribe. | `ipc_io_task` split into `run_io_loop` + reconnect wrapper. Backoff: 200 → 400 → 800 → 1600 → 5000 ms cap. Re-sends `Action::Subscribe` after reconnect. `warn!` on drop, `info!` on reconnect. Live test is `#[ignore]` (needs paired sockets); verified manually by restarting `seedd` mid-session. |
| __F6__ · Non-truecolor braille fallback advertised "8-level density mapping" but only produced 5 distinct outputs (densities 2↔3, 4↔5, 6↔7 collapsed). | `braille_to_block` now maps density 0..=8 to 8 distinct chars: `' ', ·, ░, ▀, ▒, ▄, ▓, ▊, █`. |

### Findings Carried forward

- __F2b · Quit-flush__ (TASK-009 AC: "Quit flushes pending events to daemon before exit") — `should_quit` breaks the loop and drops `IpcClient` before the I/O task drains. Fine in practice (capacity-64 mpsc, tiny bursts), but the AC is unmet.
- __F3 · Silent disconnect feedback__ — when the I/O task is dead, user verbs are dropped with only a file-log `warn!`. No toast, no log pane entry. Tracked as part of TASK-026 in [[v0-1-0-punch-list]].
- __F5 · `pick_orbit_reminders` per-frame allocations__ — two `Vec` clones, BTreeMap clone, full `GlyphFrame::clone()` on non-truecolor terminals (~4000 cells). Acceptable at 20 Hz today; profile before scaling up.
- __F8 · `Action::Subscribe` is misnamed__ — it's really `GetSnapshot`; daemon auto-subscribes to broadcasts on accept. Rename is a breaking protocol change; deferred.
- __F11 · No daemon-stop affordance from the TUI on Windows__ — by design (DETACHED_PROCESS). README troubleshooting calls out `taskkill`.
- __F16 · 3s snapshot timeout on cold start__ — late-arriving Snapshot does overwrite, but the user sees stale `initial_state` briefly. Documented in README troubleshooting.
- __F17 · `#[allow(dead_code)]` on `App` hides genuinely-unused fields__ — `config`, `seed_home`. Tracked in TASK-028 in [[v0-1-0-punch-list]] (alongside the broader carving work).

### Wire-protocol Drift Check

Field-by-field diff of `seed-daemon/src/wire.rs` vs `seed-tui/src/client.rs`: zero drift. `Message` (8 variants), `ResponseResult`, `Action` (9 variants — 10 after Wave 4.1 added `Reset`), `MAX_FRAME = 4 MiB`, framing — all identical. The duplication is a maintenance burden but not a correctness risk; closing TASK-020 removes it entirely.

## Deferrals from This Wave

- Click/mouse on orbit cards — keyboard-first; out of scope for v0.
- Orbit ellipse dashes (the JSX has 3 SVG dashed ellipses) — produces noise at terminal cell granularity.
- Time compression / tick-rate dev knob.

## See also

- [[build-log-05-docs-and-smoke]] — README + smoke test (TASK-011).
- [[v0-1-0-punch-list]] — successor work (TASK-013 snooze, TASK-019 client_log, TASK-020 wire dedup, TASK-028 carve).
- [[event-sourcing]], [[terminal-capability-ladder]] — the invariants this wave depends on.
