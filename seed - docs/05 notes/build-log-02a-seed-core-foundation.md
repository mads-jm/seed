---
date created: Tuesday, April 28th 2026, 12:00:00 pm
date modified: Wednesday, April 29th 2026, 7:53:23 am
cssclasses: []
tags:
  - note
  - build-log
  - wave-2
status: archived
---

# Build Log 02a — Seed-core Foundation

__Tasks__: TASK-002 · TASK-003 · TASK-005 · __Wave__: 2

## Scope

Domain types + pure logic, the events module, and the config module. Establishes [[pure-core]] (clock + paths injected, no I/O in the lib), [[event-sourcing]] (single fold + replay), and the first cut of the [[reminder-lifecycle]] state machine and [[tier-progression]] table.

## What Shipped

~2,145 lines across `crates/seed-core/`: `levels.rs`, `domain.rs`, `state.rs`, `events.rs`, `config.rs`, `paths.rs`, plus integration test files for each. Cargo deps: `dirs`, `toml`. Initial pass: 85/85 tests green; after the Wave 2A.1 fixes below, 114/114.

## Technical Decisions

- __ID newtypes hold `String`, not `&'static str`__ — serde cannot derive `Deserialize` for newtypes wrapping `&'static str` without a custom impl. Static catalog structs (`Category`, `Reminder`) keep `&'static str` for `static` items; runtime types use `String`-based newtypes (`TraitId`, `ReminderId`, `CategoryId`) with `From<&str>` impls.
- __OSRS XP table via `OnceLock<Vec<u64>>`__ — matches JSX `buildXpTable` bit-exact at L2/10/…/92/99. Lvl 99 = 13,034,431 (later rescaled to 1,303,443 in Wave 6, see [[build-log-06-xp-calibration]]).
- __`xp_drain()` returns `u32` scaled × 100__ — JSX returns 0.35f; integer API avoids fp drift. Callers accumulate and divide. Awkward at the boundary — flagged as a contract risk against `TraitXpChanged.delta` units (no schema annotation forces the divide).
- __`xp_reward` is deterministic__ — JSX adds `Math.random()` jitter; removed for pure logic. Daemon can re-add at the application layer.
- __Snapshot tests rely on alphabetical key ordering__ — `serde_json::Value::Object` serialises keys alphabetically when routed through `to_value` → `Map`. Snapshot strings reflect this; renames still break, but anyone routing through `to_string` directly will see different output. Documented at the test site.
- __`mid_journey_state` is deterministic__ — JSX used `Math.random()` for streaks; replaced with `i % 6` / `i % 10` so the function is pure and testable. AC says match tier/XP, not random portions.
- __Tier thresholds are an architect invention__ — JSX doesn't specify them. Set to round numbers (avg level × 9 traits). `total_level=149` (mid-journey) lands in Bloom, matching the backlog's "FROND/BLOOM" target. These thresholds are not in the locked spec; future re-tuning would silently rewrite `TierChanged` event semantics.

## Deferrals

- `events-schema.md` deferred to Wave 5 (TASK-011).
- TASK-004 (glyph renderer) split into Wave 2B.
- `last_done_ms` is not stamped in `apply_event(ReminderCompleted)` — daemon stamps it when writing the event. Avoids clock injection in the fold.

## Inspector Findings & Fixes

The Wave 2A inspector pass blocked on contracts that downstream waves would lock in. Wave 2A.1 patched the load-bearing items before Wave 2B opened.

| Finding | Fix |
|---|---|
| __F1__ · `Event::Unknown` was a unit variant — `from_envelope` dropped `kind` + `data` for unrecognised kinds, and `to_envelope(&Event::Unknown)` re-emitted `{"kind":"seed.unknown","data":null}`. A daemon round-tripping a log written by a newer client would lose every unknown event. Hollow forward-compat — see [[wire-versioning]]. | __Fix 1__: `Unknown { kind: String, data: Value }`. Removed `#[serde(other)]`. `to_envelope` special-cases `Unknown` to pass through verbatim. `from_envelope` returns `Result<Event, serde_json::Error>` — known kinds fully deserialize (errors surface), unknown kinds become `Unknown { kind, data }`. |
| __F2__ · `from_envelope` used `unwrap_or(Event::Unknown)` for known kinds with malformed `data`, masking corruption as "unknown kind". | Folded into __Fix 1__ — known-kind deserialize errors now surface as `Err`. |
| __F3__ · `apply_event(ReminderCompleted)` incremented `state.completed_total` and pushed a log entry even when `reminder_id` / `trait_id` were unknown. Forever-divergent state. | __Fix 2__: upfront existence checks; whole event is a no-op when either id is absent. |
| __F4__ · `apply_event` panicked on `u32` overflow in debug builds (and wrapped silently in release). A tampered snapshot pre-seeding `total_done = u32::MAX` would crash on the next user action. | Folded into __Fix 2__ — all increments use `saturating_add`. |
| __F5__ · `mid_journey_state` had a bare `.unwrap()` on a map lookup — the only one in `seed-core/src/` outside tests. Production code can't `unwrap` on map lookups. | __Fix 3__: replaced with `.unwrap_or_else(|| panic!(…))` naming the missing trait, plus a `# Panics` doc section explaining it's a programming-error guard on a static fixture. |
| __F6__ · `Config::load` accepted arbitrary reminder IDs (e.g. `[reminders.watter]`) and grew the catalog past 20. No catalog enforcement. | __Fix 5b__: `validate_reminder_ids` rejects unknown keys with the offending list. |
| __F7__ · `notif_style` and `snooze_min` defaults diverged from JSX baseline (`standard`/`10` vs JSX `flash`/`5`). Silent spec deviation. | __Fix 6__: matched JSX exactly. `NotifStyle::Flash` is now `#[default]`. JSX source referenced at both sites. |
| __F9__ · Snapshot test only locked one variant's JSON shape; 10 of 11 variants could be silently renamed. | __Fix 4__: 12 `snapshot_*_json_shape` tests (every variant + `Unknown`) asserting byte-exact JSON. |
| __F10__ · `Config::load` `eprintln!`'d malformed-TOML errors with ANSI escape codes — fatal in raw-mode TUI — and silently discarded *all* config on a single typo. | __Fix 5a__: returns `Err(anyhow::Error)` with context. No stderr writes from library code. |
| __F12__ · `active_hours` accepted any `(u8, u8)` — `[99, 200]` parsed cleanly. | __Fix 5c__: `validate_active_hours` enforces `0..=23` and `start ≤ end`. |
| __F16__ · `unsafe { std::env::set_var }` in a lib-internal test was safe today but would flake the moment any other test read `SEED_HOME`. | __Fix 7__: moved to a dedicated integration test file (separate binary, single-threaded). |

### Findings Still Open

These were called out but not patched in 2A.1; carried forward as risks the downstream waves were aware of:

- __F8__ — `xp_drain` × 100 scale is undocumented in the wire schema (`TraitXpChanged.delta` has no unit annotation). Cross-component drift risk; closed in Wave 5 when [[events-schema]] landed.
- __F11__ — Duplicate `seed_home` API (`paths::seed_home` and `config::seed_home_path`). Tracked as TASK-018 in [[v0-1-0-punch-list]] (which is also the work that restores the [[pure-core]] invariant for `seed_core::config`).
- __F13__ — `level_for_xp` is O(99); `binary_search` would close it. Hot path is fine in practice; left as a perf nit.
- __F15__ — Tier thresholds remain a free-form invention rather than a locked spec.
- __F17–F23__ — Non-blocking shape/style/coverage nits (silent `ConfigChanged` ignores, `State::log` unbounded `VecDeque`, `to_envelope` swallowing serialise errors, missing `apply_event` test coverage). Most are absorbed by later waves; the residue is in the [[v0-1-0-punch-list#Minor / nit cleanup batch|nit batch]].

## See also

- [[build-log-02b-glyph-renderer]] — Wave 2B (TASK-004) opened after these fixes landed.

