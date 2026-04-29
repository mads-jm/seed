---
date created: Monday, April 27th 2026, 12:00:00 pm
date modified: Wednesday, April 29th 2026, 7:53:22 am
cssclasses: []
tags:
  - note
  - build-log
  - wave-6
  - redlines
status: archived
---

# Build Log 06b — Redline Fixes

__Date__: 2026-04-27 · __Wave__: 6 · __Predecessor__: [[build-log-06-xp-calibration]]

## Scope

All redline findings from the inspector review of Wave 06 (XP calibration + prestige pre-wiring). The blocker fix (C1) was a load-bearing [[event-sourcing]] correctness bug — `cumulative_levels_gained` literally never incremented in production, so tokens never accrued. Tests + clippy + fmt clean post-fixes; binaries linked from `cargo check` (real `.exe`s were locked by running processes during the fix-pass).

## Fixes

### C1 — `cumulative_levels_gained` Never Incremented in Production

`LevelUp` fold computed `old_level = level_for_xp(state.traits[trait_id])`, but `ReminderCompleted` ran first in the same commit batch and had already written `new_xp` into `state.traits`. By the time `LevelUp` folded, `level_for_xp(*xp) == new_level`, so `levels_gained = 0`. Tokens never accrued.

__Fix__: Added `old_level: u8` to the `LevelUp` event payload. The fold now uses the payload, never reads pre-event state. The daemon's `build_complete_events` computes `old_level = level_for_xp(current_xp)` before building events, when pre-completion XP is still in hand. `#[serde(default)]` keeps old log entries deserializing cleanly (they fall back to `effective_old = new_level - 1` — safe undercount). No version bump (additive field per [[wire-versioning]]).

### C2 — Late / Overdue Multipliers Never Fired

`build_complete_events` hardcoded `XpRewardOpts { on_time: true, overdue: false }`. __Every real completion since the daemon shipped paid the 1.0 (on-time) rate regardless of state.__ Users completing overdue reminders were underrewarded (1.0 vs 1.4); users completing early were overrewarded (1.0 vs 0.6).

__Fix__: Compute `XpRewardOpts` from `reminder_status_with_interval(rt.interval_min, rt.last_done_ms, rt.enabled, now_ms)` using pre-completion `last_done_ms`. `Overdue → overdue: true`; `Due → on_time: true`; `Dormant`/`Off` (early completion) → both false (0.6× late penalty). Validated via the `--ignored` smoke test; a unit test against `build_complete_events` directly is impractical without removing the `&self` bound.

### C3 — `TraitIntegrated` Overwrote instead of Incrementing

`*count = *new_integrations` trusted the payload verbatim. A malformed event could rewrite the integration count.

__Fix__: `*count = count.saturating_add(1)`. The `new_integrations` field is retained as audit trail; the fold ignores it for the actual mutation. `#[cfg(debug_assertions)] eprintln!` on mismatch flags drift without a `tracing` dep. New test: `integrate_twice_increments_to_two`.

### C4 — Clippy not Clean with `--all-targets`

- `tests/events.rs:380`: `get(…).is_none()` → `!contains_key(…)`.
- `view/tweaks.rs` (6 instances): `Default::default()` + field reassignments → struct literal with `..Default::default()`.

### M5 — Pacing Band: Per-trait, not One Wide Band

User direction: "variance being wide is okay; just define a broader contract and ensure it makes sense with the difficulty / friction of a skill."

__Fix__: Replaced the single `[3000, 6000]`-for-all approach with per-trait bands derived from each trait's fire profile. The test `pacing_band_per_trait` drives a `(trait, lo, hi)` table. `clarity` and `space` get the widest ([3000, 6000]) because their 4-hour anchors (`sun`, `rest`) are structurally incompatible with a tight daily band while still being within contract at the per-reminder XP/hr level. A 15% cadence drift in any reminder would fail even the widest band. Spec table updated in [[xp-pacing]].

### M6 — `Reset` Wiped Prestige State

User direction: "Think of prestige as its own gain; we'll eventually tune back non-prestige visuals and have permanent visual gain on prestige."

__Fix__: `Action::Reset` snapshots prestige state (`cumulative_levels_gained`, `tokens_spent`, `active_focus`, `trait_integrations`, `trait_enhancements`) before building `initial_state`, then carries them forward. Per-trait XP and reminder runtimes are reset. Load-bearing guarantee documented in [[prestige-focus]].

### M7 — Epsilon Widened 0.01 → 0.02 Unnecessarily

Reverted: actual ratio is 0.5007, well within 0.01. Tightened back in both `levels.rs` (inline test) and `tests/levels.rs` (external).

### M8 — `FocusPhaseActivated` Accepted Duplicate Trait IDs

__Fix__: Duplicate-detection loop in the fold using a `BTreeSet`. Rejects and logs on first duplicate; token is not spent. New test.

### M9 — `FocusPhaseActivated` Accepted Unknown Trait IDs

__Fix__: Existence check after the duplicate check, matching the defensive pattern from `ReminderCompleted`. New test.

### Read-twice in `build_complete_events`

`self.state.read().await` was being held twice — once for the main fold, once to compute `new_balance` for `FocusTokenEarned`. Consolidated into a single read that captures `current_xp`, `rt`, `xp_opts`, `cumulative_before`, `tokens_spent`, `active_focus`. Doc comment names the single-writer invariant at `cmd_rx.recv()`.

## Key Decisions

- __C1 fix via payload field, not fold restructure__ — adding `old_level` to `LevelUp` keeps the fold pure (`&mut State → ()`) and the wire format extensible. Restructuring to return `Vec<Event>` from the fold would have been a much bigger surface change.
- __M6 prestige preservation copies by value before write__ — a concurrent snapshot between the read and write would see pre-reset state. Acceptable given the daemon is single-writer at `cmd_rx.recv()`.

## See also

- [[build-log-06-xp-calibration]] — the wave this corrects.
- [[v0-mvp]] — consolidated milestone.
- [[v0-1-0-punch-list]] — successor backlog (the `eprintln!` from C3/m9 is in the [[v0-1-0-punch-list#Minor / nit cleanup batch|nit batch]]).
- [[event-sourcing]], [[wire-versioning]] — invariants C1 and C2 restore.

