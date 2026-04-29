---
date created: Tuesday, April 28th 2026, 9:00:00 am
date modified: Wednesday, April 29th 2026, 7:53:25 am
cssclasses: []
tags:
  - spec
  - scheduling
  - reminders
  - lifecycle
status: implemented
---

# Overdue Lifecycle Rollover

Bound the `Overdue` state in [[reminder-lifecycle]] so missed cycles eventually expire instead of accumulating. Adds the `seed.reminder.skipped` event (with a `was_snoozed` flag for snooze-leniency), a tick-time threshold, and a per-trait `traits_skipped` aggregation surfaced in LEVELS + skill-detail. No `ReminderState` enum widening.

## Why

The [[reminder-lifecycle]] machine is `Off / Dormant / Due / Overdue` and `Overdue` persists indefinitely until the user manually completes, snoozes, or disables the reminder. After a sleep window or a few unattended hours the TUI greets the user with a wall of muted-red `OVRD` cards across the orbit pane, side-panel LIST tab, and skill-detail overlay. XP also drains continuously while overdue with no upper bound.

Recurring reminders are habits, not nags. A missed cycle should expire, the reminder should roll forward to its next opportunity, and the visible state should return to "upcoming" without user intervention.

## Contract

### Threshold

A reminder rolls forward when `now_ms − last_done_ms > 2 × interval_ms`. Equivalently, when `overdue_ms > interval_ms` using the value already computed in `reminder_status_with_interval` (`crates/seed-core/src/domain.rs`).

This caps the OVRD-visibility window at exactly `0.5 × I` per cycle:

| reminder | interval | OVRD window |
|---|---|---|
| align | 30 min | 15 min |
| breathe | 25 min | 12.5 min |
| water | 45 min | 22.5 min |
| stand | 50 min | 25 min |
| sun · rest | 4 h | 2 h |
| sit | 6 h | 3 h |
| journal · reflect | 24 h | 12 h |

The rule is uniform across reminders. Per-reminder grace overrides are deferred (see [Out of scope](#out-of-scope-deferred)).

### State Machine

```
Off
Dormant   [0,    1.0×I)
Due       [1.0×I, 1.5×I)
Overdue   [1.5×I, 2.0×I)        ← max OVRD window
                ↓ Skip event ↓
Dormant   [0,    1.0×I)         (cycle restarts)
```

The enum stays four-valued. Skip is a daemon-side scheduler decision — it is not a derived state.

### Skip Emission

The scheduler tick (`crates/seed-daemon/src/schedule.rs`) checks the threshold inside the existing `ReminderState::Overdue` arm. When the threshold is exceeded, the tick emits exactly one `ReminderSkipped` event for the reminder and short-circuits — no XP drain that tick.

```rust
ReminderState::Overdue => {
    let interval_ms = rt.interval_min as i64 * 60 * 1000;
    let overdue_ms = -status.ms_left;

    if overdue_ms > interval_ms {
        let missed = (overdue_ms / interval_ms) as u32;
        events.push(Event::ReminderSkipped { … });
        continue;
    }

    if !in_active_hours { continue; }   // active-hours drain gate
    // … existing XP drain …
}
```

A long-running daemon catches up tick-by-tick. A cold-boot daemon catches up on its first tick — even a 10×I gap is collapsed in a single Skip event with `missed_cycles = 9`.

### Active-hours Drain Gate

XP drain is now gated by `in_active_hours` so overnight idleness no longer bleeds XP. This was previously ungated (the original TASK-007 wording explicitly distinguished notification gating from drain gating); the change folds the two gates together. Notification gating is unchanged. Skip emission is __not__ gated — state cleanup must run during sleep so the user wakes to a clean board.

### apply_event Semantics

```
state.reminders[id].last_done_ms = at_ms              // always
// only when was_snoozed = false:
state.reminders[id].streak       = 0
state.reminders[id].total_missed += 1                 // saturating
state.traits_skipped[trait].lifetime += 1             // saturating
state.traits_skipped[trait].recent.push(at_ms)        // pruned to last 7d
```

The reminder's next status compute returns `Dormant` with `ms_left == interval_ms`. Streak resets and per-trait skip aggregation populate __only when the user did not snooze during the cycle__ — see [Snooze leniency](#snooze-leniency). `total_missed` (already on `ReminderRuntime`, previously never written) and the new `traits_skipped` map both start being populated by this change.

### TUI Behaviour

The three OVRD render sites — `crates/seed-tui/src/view/{orbit,side_panel,skill_detail}.rs` — are unchanged. Once a Skip event lands and the state mirror catches up, the reminder reads as Dormant and the OVRD label disappears organically.

## Data Model

### Event

```rust
Event::ReminderSkipped {
    at_ms: i64,                        // alphabetical order matches serde_json output
    missed_cycles: u32,                // (overdue_ms / interval_ms) as u32 at emission
    reminder_id: ReminderId,
    #[serde(default)]
    was_snoozed: bool,                 // see Snooze leniency
}
```

Wire kind: `seed.reminder.skipped`. Schema rule per [[wire-versioning]]: adding an event variant is non-breaking; older readers preserve it via `Event::Unknown` round-trip. Documented in [[events-schema]]. The fold mutation is one more arm in the [[event-sourcing]] `apply_event` function.

### Runtime Field

`ReminderRuntime.total_missed: u32` already exists in `crates/seed-core/src/state.rs` and is never written today. This change starts populating it. No struct migration.

### Daemon Helpers

None. The threshold check is inline in the scheduler tick — it reuses `status.ms_left` from the existing `reminder_status_with_interval` call and a one-line `interval_ms` recompute.

## Implementation order

1. This spec.
2. Add `Event::ReminderSkipped` variant in `crates/seed-core/src/events.rs` with serde `rename = "seed.reminder.skipped"` and `was_snoozed: bool` (with `#[serde(default)]`). Extend `is_known_kind`, `event_kind`, `apply_event`. Add `TraitSkipStats` and `state.traits_skipped` in `state.rs`.
3. Modify the `ReminderState::Overdue` arm in `crates/seed-daemon/src/schedule.rs`: emit Skip when over 2×I (with `continue`), passing `was_snoozed = rt.snoozed_until_ms > rt.last_done_ms`; else gate drain by `in_active_hours`.
4. Document the kind in [[events-schema]] (envelope shape, fields, threshold semantics, canonical effect, snooze-leniency split).
5. Tests:
   - `crates/seed-core/src/events.rs`: round-trip (both `was_snoozed` true and false), byte-equal JSON snapshot, `apply_reminder_skipped_*` (resets `last_done_ms`, zeroes streak, increments `total_missed` and per-trait skip aggregation when `was_snoozed = false`; preserves streak/missed when `was_snoozed = true`; unknown id is no-op).
   - `crates/seed-daemon/src/schedule.rs`: split existing `overdue_reminder_drains_xp` into `overdue_under_2x_drains_xp` (last_done = now − 80 min) and `overdue_over_2x_emits_skip_not_drain` (last_done = now − 100 min); new `auto_skip_reports_correct_missed_cycles`, `auto_skip_during_inactive_hours_still_fires`, `overdue_drain_gated_by_active_hours`, `auto_skip_then_status_returns_dormant`, `auto_skip_emits_was_snoozed_true_when_snooze_active_in_cycle`, `auto_skip_emits_was_snoozed_false_when_no_snooze_in_cycle`.

## Snooze Leniency

`Event::ReminderSkipped` carries a `was_snoozed: bool` field (`#[serde(default)]` so older log entries without it deserialize as `false`).

The daemon computes it at emission: `rt.snoozed_until_ms > rt.last_done_ms` — true if the user explicitly deferred at any point during this overdue cycle, regardless of whether the snooze has since expired.

`apply_event` semantics:
- `rt.last_done_ms = at_ms` — __always__ (state cleanup is unconditional).
- `rt.streak = 0` — __only if `!was_snoozed`__.
- `rt.total_missed += 1` (saturating) — __only if `!was_snoozed`__.

Skip still fires at 2×I regardless — the snooze flag only affects the penalty, not whether rollover occurs. Design intent: coax users into consistency with a reasonable penalty, but keep friction low.

## Per-trait Skipped Surface

`State` carries a `traits_skipped: BTreeMap<TraitId, TraitSkipStats>` field (`#[serde(default)]` for old-snapshot compat). `TraitSkipStats` holds:
- `lifetime: u32` — total non-snoozed skips since awakening.
- `recent: Vec<i64>` — epoch-ms timestamps of recent skips, pruned at apply time to the last 7 days.

`apply_event` populates this when `!was_snoozed`: walks the static catalog (reminder → category → trait), increments `lifetime`, appends `at_ms` to `recent`, then prunes entries older than `at_ms − 7d`.

`TraitSkipStats::count_7d(now_ms)` counts entries within the 7-day window relative to now.

Two TUI render sites surface this:

__LEVELS tab__ — each per-trait row appends a `▾N` indicator after the XP line when `count_7d > 0`. Color: `palette.fg` (muted) for 1–2 skips; `palette.due` (amber) for 3+.

__Skill detail overlay__ — adds a `Skipped: N (7d) · M lifetime` line after the reminder list. Omitted entirely when `lifetime == 0`. When `count_7d == 0` but `lifetime > 0`, shows `Skipped: M lifetime` only. Same color tiers as LEVELS.

## Out of Scope (deferred)

### Manual Skip Action

A user-facing `Action::Skip { reminder_id }` for voluntary dismissal of a Due or Overdue reminder. Wiring is parallel to `Action::Snooze` in `crates/seed-daemon/src/daemon.rs`. The auto-skip path always increments `total_missed` when `was_snoozed = false`; __manual skip should likely not__ — voluntary acknowledgement is not a missed cycle. That asymmetry argues for either a `kind: SkipKind { Auto, Manual }` field on the event, or two separate event variants (`ReminderSkipped` for auto, `ReminderDismissed` for manual). Decide when wiring the action.

### Per-reminder Grace Override

The uniform `2 × I` rule gives long-cadence reminders generous OVRD windows: `journal` and `reflect` at 24-hour interval present 12 hours of OVRD before rollover. If lived experience shows that's too long for daily anchors — e.g. you should not see "OVRD" on yesterday's morning pages at noon today — add an optional `grace_min: Option<u32>` to the static `Reminder` catalog and use `grace_min.unwrap_or(interval_min) as i64 * 60 * 1000` in the threshold check. Defer until lived signal.

### Streak Rules beyond Skip

Today `streak` only ever advances forward, via `ReminderCompleted`. This change adds the first reset path (auto-skip with `was_snoozed = false` → `streak = 0`). A wider streak-policy review is its own design question:

- Does completing late but before Skip preserve streak?
- Does an explicit user snooze that crosses an active-hours boundary count as a missed day?
- Are streaks per-reminder, per-trait, or per-day?

None of those are answered here.

## Risks

- __Pacing is unchanged but XP drain becomes finite.__ The existing pacing band test in `crates/seed-core/tests/levels.rs` is computed from on-time completion budgets and never models drain, so it does not reject this change. But effective XP-per-day under partial-adherence shifts upward (drain bounded to `0.5 × I` per missed cycle, gated to active hours). The 1-year-to-99 contract from [[xp-pacing]] is a perfect-adherence ceiling and stays correct; the median experience moves slightly closer to it.
- __`missed_cycles` for cold-boot replay.__ A cold daemon that finds a 10×I gap emits one Skip event with `missed_cycles = 9`. Replays produce identical state. If retroactive backfill of historical state is later added (e.g. importing legacy logs), care will be needed to ensure `at_ms` for the synthesized Skip events lines up correctly — but no such backfill is planned.
- __Daily-anchored OVRD remains visible for half a day.__ See [Per-reminder grace override](#per-reminder-grace-override). Acceptable for v1.

## See also

- [[xp-pacing]] — the on-time XP curve this rollover implicitly preserves
- [[prestige-focus]] — token earnings derive from cumulative levels and are unaffected
- [[prestige-integrate]] — per-trait XP reset is unaffected
- [[reminder-lifecycle]] — the state machine this spec extends with the auto-skip transition
- [[event-sourcing]] — the fold semantics that make the `was_snoozed` split tractable
- [[cli-flags]] — `SEED_DEV=1` TWEAKS tab is the manual lever for backdating `last_done_ms` to test rollover end-to-end
