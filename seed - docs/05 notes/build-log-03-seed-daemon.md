---
date created: Tuesday, April 28th 2026, 12:00:00 pm
date modified: Wednesday, April 29th 2026, 7:53:23 am
cssclasses: []
tags:
  - note
  - build-log
  - wave-3
status: archived
---

# Build Log 03 — Seed-daemon

__Tasks__: TASK-006 · TASK-007 · __Wave__: 3

## Scope

The daemon: IPC server, state owner, event-log writer (TASK-006); scheduler tick + OS notifications (TASK-007). This is where [[event-sourcing]] meets the wire — `commit()` is the single writer, [[snapshot-and-replay]] is the startup pattern, and the [[reminder-lifecycle]] tick lives in `schedule.rs`. Initial pass: 155/155 tests green; after Wave 3.1 fixes, 158/158.

## What Shipped

| Crate | Files |
|---|---|
| `seed-core` | New `Event::ReminderNotified` variant + fold; `ReminderRuntime.last_notified_ms` field (`#[serde(default)]`); 4 tests. |
| `seed-daemon/src/` | `wire.rs` (Message/Action types, length-prefixed JSON framing with 4 MiB cap), `event_log.rs` (append-only JSONL with fsync, atomic snapshot, corrupt-line tolerance), `notify.rs` (`Notifier` trait + `DesktopNotifier` + `MockNotifier`), `schedule.rs` (tick fn with injected clock), `ipc.rs` (`run_listener`, `probe_existing`), `daemon.rs` (Daemon struct, snapshot+replay startup, periodic snapshot every 100 events / 5 min), `main.rs` (CLI flags, tracing, Ctrl-C handler). |
| Cargo deps | `serde`, `serde_json`, `chrono`, `dirs`, `interprocess`, `notify-rust`, `async-trait`; `tempfile`, `pretty_assertions` (dev). |

## Technical Decisions

- __`Notifier` trait + `MockNotifier`__ — `notify-rust::Notification::show()` is never called in tests. `DesktopNotifier` wraps in `spawn_blocking`, swallows errors as `warn!` rather than propagating. The daemon never crashes on a failed OS notification.
- __`ReminderNotified` event for debounce__ — rather than mutable daemon state outside the event log, we emit `ReminderNotified` per fire (keeping the [[event-sourcing]] invariant that all state changes flow through `apply_event`). The fold stores `last_notified_ms` on the runtime. Debounce check: `last_notified_ms > last_done_ms` — silenced for the entire current due window until completion bumps `last_done_ms`. (Wave 3.1 wired the `last_done_ms` bump that this scheme depends on; see fixes below.)
- __`#[serde(default)]` on `last_notified_ms`__ — old snapshots / event logs deserialize as `0` (= never notified) rather than crashing. Forward-compat freebie.
- __XP drain `max(1)`__ — `xp_drain()` returns 35 (= 0.35 ×100). Integer division to `35 / 100 = 0` would make the mechanic invisible. Floor at 1 XP per overdue tick. Exact fractional accumulation deferred.
- __Periodic timer reuses the action channel__ — the 30s tick sends `Action::TriggerReminderNow { reminder_id: None }` through the same command channel, keeping the daemon's main loop to one `match`. `request_id = 0` signals "no response needed."
- __`interprocess` `GenericNamespaced`__ — Windows pipe at `\\.\pipe\seed-daemon-<USERNAME>`, Unix abstract socket. `socket_name()` is cfg-guarded. Both are OS-cleaned on process exit, so stale-socket cleanup is a non-issue in this config.
- __Atomic snapshot__ — `write → .tmp → rename`. Windows `fs::rename` over an existing file can fail if the target is locked; for v0 acceptable, and far safer than overwriting in place. (The non-atomic Windows case is tracked as TASK-027 in [[v0-1-0-punch-list]].)
- __Active-hours boundary is exclusive end__ — `now_hour >= start && now_hour < end`. Hour 22 is excluded from `(7, 22)`, matching the JSX reference.

## Inspector Findings & Fixes (Wave 3.1)

| Finding | Fix |
|---|---|
| __F1__ · `apply_event(ReminderCompleted)` never set `last_done_ms`. The Overdue branch had no debounce, so a completed-but-still-Overdue reminder bled 1 XP every 30s for the rest of the session — 2,880×/day. The build-log's "may re-notify within the same tick" understated it; this was persistent XP bleed. | Added `at_ms: i64` to `Event::ReminderCompleted`; the fold stamps `rt.last_done_ms = at_ms`. `build_complete_events` passes `now_ms`. New tests assert stamping + replay determinism. |
| __F2__ · Response routing was completely broken. `broadcast_response` wrapped `Message::Response` inside `Event::Unknown { kind: "__response__", … }` and broadcast it as a `StateDiff` to every client. No code path unwrapped it — responses were silently discarded, and the bogus events polluted every subscriber's `apply_event` stream. `Subscribe` also broadcast a fake `CompanionAwakened` to *all* clients on every new connection. | Per-connection response channel. Each connection gets `(resp_tx, resp_rx): mpsc::channel<Message>(64)`. `Command::Action` carries `resp_tx`; `handle_action` calls `send_response(resp_tx, request_id, result)` directly to the originator. New `Command::Disconnect { conn_id }` cleans up the daemon's `conn_resp` map. `Subscribe` now sends `Message::Snapshot` privately. |
| __F3__ · The Ctrl-C handler called `std::process::exit(0)` directly. `Daemon::shutdown()` (final snapshot, log flush) was never reached. The build-log's "graceful shutdown … flushes log" claim was false; only fsync-per-event saved the log. | `oneshot::channel<()>` between the signal handler and the daemon. Ctrl-C sends; `Daemon::run_with_shutdown` selects on the signal arm, breaks the loop, and `shutdown()` runs in the normal exit path. New public entry point: `run_with_shutdown(seed_home, foreground, shutdown_rx)`. |
| __F5__ · `validate_active_hours` rejected `start > end` but not `start == end`. `(7, 7)` parsed cleanly and silently disabled all notifications. | Now rejects `start >= end` with an error that explicitly names the zero-length-window failure mode. New test. |
| __F7__ · `Action::TriggerReminderNow { reminder_id: Some(id) }` discarded the id and just ran the normal scheduler tick — indistinguishable from waiting 30s. | When `reminder_id: Some(id)`, bypasses debounce + active hours and fires that reminder's notification immediately. `None` path is unchanged. |
| __F8__ · No supervisor for the three spawned tasks (IPC listener, tick timer, snapshot timer). A panic deep in any of them would silently kill that subsystem with no log; notifications could just stop. | `run_inner`'s `tokio::select!` now watches all three `JoinHandle`s (pinned across loop iterations). Unexpected exit logs `error!` and triggers graceful shutdown. |

### Findings Carried forward

- __F4__ — `path.with_extension("tmp")` produces `snapshot.tmp`, not `snapshot.json.tmp`. Behaviour correct (atomic), docstring lied; fixed in commentary later.
- __F6__ — Snooze silences the whole due window: snooze sets `snoozed_until_ms` but `last_notified_ms > last_done_ms` stays true forever (until Complete bumps `last_done_ms`), so no second notification fires after snooze expires. Tracked under the snooze workstream — see [[overdue-rollover]] and TASK-013 in [[v0-1-0-punch-list]].
- __F9__ — `commit()` holds the state write-lock and event-log mutex across fsync. Slow disk stalls all readers. Acceptable at v0 scale.
- __F10__ — Spawn-daemon integration test deferred to Wave 5 (TASK-011), where it landed as `tests/smoke.rs`.

## Deferrals from This Wave

- Per-connection response channels — *fixed in 3.1* (was originally deferred to Wave 4).
- Exact fractional XP drain — Wave 4+.
- Integration test — Wave 5 ([[build-log-05-docs-and-smoke]]).

## See also

- [[build-log-04-seed-tui]] — TUI client built against this IPC contract.
- [[overdue-rollover]] — later spec evolving the Overdue → drain branch into auto-skip.
- [[event-sourcing]], [[snapshot-and-replay]], [[reminder-lifecycle]] — the invariants this wave wires up.

