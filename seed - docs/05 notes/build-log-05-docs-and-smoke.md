---
date created: Tuesday, April 28th 2026, 12:00:00 pm
date modified: Wednesday, April 29th 2026, 7:53:22 am
cssclasses: []
tags:
  - note
  - build-log
  - wave-5
status: archived
---

# Build Log 05 — Docs and Smoke

__Tasks__: TASK-011 · __Wave__: 5

## Scope

README, the wire-schema doc, and the end-to-end smoke test that ties the whole thing together. This wave was the v0 ship gate — inspector recommended SHIP after this pass. The smoke test exercises [[snapshot-and-replay]] end-to-end (real `seedd` against a `tempfile::TempDir`).

## What Shipped

- `README.md` — full v0 README (why / install / quickstart / commands / config / troubleshooting / pour integration / tech stack / dev).
- `events-schema.md` — every `Event` variant documented with locked JSON examples derived from the snapshot tests. (Now lives at [[events-schema]] post-migration.)
- `crates/seed-daemon/src/lib.rs` — new `[lib]` target exposing `wire`, `ipc`, `event_log`, `daemon`, `notify`, `schedule`. `main.rs` switched to importing through the lib.
- `crates/seed-daemon/tests/smoke.rs` — end-to-end: spawns real `seedd` against a `tempfile::TempDir`, drives Subscribe → Complete → Shutdown, asserts `events.jsonl` contains the expected envelope. `#[ignore]` by default; runs explicitly with `cargo test --test smoke -- --ignored`.

## Technical Decisions

- __Daemon → lib + bin__ — the smoke test needed `seed_daemon::wire::*` and `seed_daemon::ipc::socket_name`. Converting to a lib+bin pair was cleaner than duplicating the wire types a third time. (Note: the TUI's `client.rs` still has its own copy from Wave 4 — TASK-020 in [[v0-1-0-punch-list]] closes that loop.)
- __`Hello` drain in the smoke probe__ — the daemon sends a `Hello` frame on every accept. The original probe sent `Ping` immediately and read `Hello` back, mismatching `Pong` and timing out at 5s even on a healthy daemon. Probe now reads-and-matches `Hello` first, then sends `Ping`.
- __Smoke is `#[ignore]` by default__ — running it requires the `seedd` binary built in the same `target/` profile; `cargo test --workspace` doesn't guarantee that for integration tests. Explicit invocation surfaces in CI as a separate gate.

## Inspector Findings

The final-sweep inspector recommended __SHIP v0__ with nothing blocking. Findings carried forward into [[v0-1-0-punch-list]]:

| Finding | Disposition |
|---|---|
| __N1__ · `debug_space.rs` scratch file at repo root, not in `Cargo.toml` | Cleanup. |
| __N2__ · `events-schema.md` claimed "daemon skips events with unknown major `v`" — `event_log::load_from` never inspects `env.v`. Either implement the check or soften the doc. The [[wire-versioning]] policy as documented requires this to actually work. | Tracked under TASK-023 in [[v0-1-0-punch-list]]. |
| __N3__ · Build-log claimed "221 passed" — actual `cargo test --workspace` is 218 + 5 ignored; the smoke `#[ignore]` doesn't add to the workspace count. | Corrected in this distillation. |
| __N4__ · Duplicate `seed_home` resolver: `seed_core::paths::seed_home()` and `seed_core::config::seed_home_path()`. The `config` copy has zero callers. The fix also restores [[pure-core]] for that module. | Tracked under TASK-018 in [[v0-1-0-punch-list]]. |
| __N5__ · Carry-forward UX gaps — quit-flush, disconnect toast, `Action::Subscribe` misnomer (it's really `GetSnapshot`). | All three carried into [[v0-1-0-punch-list]] (TASK-009 / TASK-026). |
| __N6__ · Smoke test leaves a daemon `WARN response channel closed before send request_id=3` because the client disconnects before the Shutdown response is sent. Expected given the protocol; either wait for Response-for-Shutdown or suppress that specific warn. | Cosmetic; left as-is. |
| __N7__ · `01-workspace-bootstrap.md` lacked an "Inspector Findings" section that every other wave had. | Style-only. |
| __N8__ · `async-trait` declared in both `[workspace.dependencies]` and `seed-daemon/Cargo.toml` direct-dep. The direct version takes effect; the workspace entry is dead. | Hygiene; folded into the [[v0-1-0-punch-list#Minor / nit cleanup batch|nit batch]]. |

## V0 Scope Summary

__Shipped__: full Cargo workspace; 9 traits × 20 reminders; OSRS curve 1–99; SEED → ZENITH tier progression (10 tiers); event-sourced state with snapshot + tail-replay; 5 palettes with truecolor + 256 + ASCII fallbacks; tweaks panel with two-step reset; auto-reconnect with backoff; panic hook + `TerminalGuard`; rotating logs; `seed init` scaffold; smoke test; 218 unit/integration tests + 1 smoke (ignored).

__Deferred__ to the v0.1.0 punch list and beyond: quit-flush, disconnect toast, `Action::Subscribe` rename, fractional XP-drain accumulator, time-compression in tweaks, prebuilt install scripts, click/mouse on orbit cards, pour integration. All carried in [[v0-1-0-punch-list]] or in the post-Wave-6 prestige work.

## See also

- [[build-log-06-xp-calibration]] — Wave 6, post-ship recalibration of the XP economy + prestige pre-wiring.
- [[v0-mvp]] — consolidated milestone these notes feed into.
- [[snapshot-and-replay]], [[wire-versioning]] — invariants the smoke test and the events-schema doc depend on.
