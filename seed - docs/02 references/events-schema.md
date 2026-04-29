---
date created: Tuesday, April 28th 2026, 12:00:00 pm
date modified: Wednesday, April 29th 2026, 7:53:27 am
cssclasses: []
tags:
  - reference
  - events
  - wire
  - schema
status: implemented
---

# Events Schema

The event log at `~/.seed/events.jsonl` is the durable source of truth for reminder completions, level-ups, and state changes — `seed`'s [[event-sourcing]] substrate. Each line is a JSON envelope:

```json
{ "v": 1, "ts": "<rfc3339 timestamp>", "kind": "seed.<namespace>.<event>", "data": { ... } }
```

Fields:
- `v`: envelope version (currently `1`). Breaking schema changes bump this.
- `ts`: when the event happened, RFC 3339 UTC.
- `kind`: dot-namespaced string. All current events are under `seed.*`.
- `data`: payload specific to the event kind. Unknown kinds are preserved verbatim on deserialize (via `Event::Unknown`) so downstream tools never lose data — the [[wire-versioning]] forward-compat invariant.

## Events

### `seed.reminder.completed`

User (or auto-trigger) completed a reminder.

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.reminder.completed",
  "data": {
    "at_ms": 1745000000000,
    "new_xp": 1234,
    "reminder_id": "water",
    "streak": 5,
    "trait_id": "flow",
    "xp_gained": 74
  }
}
```

Fields:
- `reminder_id`: the reminder completed (e.g. `"water"`, `"walk"`, `"breathe"`).
- `trait_id`: the wellness trait that received XP (e.g. `"flow"` for hydration).
- `xp_gained`: XP awarded for this completion.
- `new_xp`: total XP for `trait_id` after the award.
- `streak`: consecutive completion count for this reminder.
- `at_ms`: epoch milliseconds when the completion was recorded. Redundant with envelope `ts` but available for fast reads without timestamp parsing.

### `seed.reminder.notified`

Daemon fired an OS notification for a due reminder.

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.reminder.notified",
  "data": {
    "at_ms": 1745000000000,
    "reminder_id": "water"
  }
}
```

Fields:
- `reminder_id`: the reminder that was notified.
- `at_ms`: epoch milliseconds when the notification fired. Used internally by the scheduler as a debounce marker — no second notification fires for the same due window.

### `seed.reminder.skipped`

Daemon auto-skipped an overdue reminder that exceeded 2× its interval since `last_done_ms`. Rolls the reminder forward to a fresh Dormant cycle. Emitted regardless of active hours — state cleanup must run during sleep so the user wakes to a clean board.

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.reminder.skipped",
  "data": {
    "at_ms": 1745000000000,
    "missed_cycles": 3,
    "reminder_id": "water",
    "was_snoozed": false
  }
}
```

Fields:
- `at_ms`: epoch milliseconds when the skip was recorded. Written to `last_done_ms` so the next status compute returns Dormant with `ms_left == interval_ms`.
- `missed_cycles`: number of full interval cycles that elapsed beyond the first (`(overdue_ms / interval_ms) as u32`). At exactly 2×I this is 1; at 10×I this is 9. Informational — not used in the fold.
- `reminder_id`: the reminder that was auto-skipped.
- `was_snoozed`: `true` when the user snoozed at some point during this overdue cycle (`snoozed_until_ms > last_done_ms` at emission time). Suppresses the streak reset and `total_missed` increment — the user signalled intent, so the penalty is waived. `#[serde(default)]` so older log entries without this field deserialize as `false`.

Threshold: the scheduler fires this event when `now_ms − last_done_ms > 2 × interval_ms` (equivalently, `overdue_ms > interval_ms` where `overdue_ms = −ms_left`). Each tick emits at most one `ReminderSkipped` per reminder; skip and XP drain are mutually exclusive within a tick.

Canonical effect:
- `state.reminders[reminder_id].last_done_ms ← at_ms` (always)
- `state.reminders[reminder_id].streak ← 0` (when `was_snoozed = false`)
- `state.reminders[reminder_id].total_missed += 1` (saturating; when `was_snoozed = false`)
- `state.traits_skipped[trait_id].lifetime += 1` (when `was_snoozed = false`)
- `state.traits_skipped[trait_id].recent` appended and pruned to last 7 days (when `was_snoozed = false`)

### `seed.reminder.snoozed`

User deferred a reminder for a configurable period.

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.reminder.snoozed",
  "data": {
    "reminder_id": "look",
    "snooze_min": 10,
    "until_ms": 9999999
  }
}
```

Fields:
- `reminder_id`: the reminder snoozed.
- `snooze_min`: duration of the snooze in minutes.
- `until_ms`: epoch milliseconds when the snooze expires.

### `seed.reminder.interval_changed`

User changed the cadence for a single reminder. Persisted via the event log and replayed by `apply_event` into `ReminderRuntime::interval_min`, so cadence overrides survive restarts without depending on `~/.seed/config.toml`.

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.reminder.interval_changed",
  "data": {
    "minutes": 60,
    "reminder_id": "water"
  }
}
```

Fields:
- `reminder_id`: the reminder whose interval was changed.
- `minutes`: the new interval in minutes. Replaces the previous `interval_min` for this reminder; the static catalog default is unchanged.

### `seed.reminder.enabled` / `seed.reminder.disabled`

A reminder was toggled on or off.

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.reminder.enabled",
  "data": { "reminder_id": "sit" }
}
```

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.reminder.disabled",
  "data": { "reminder_id": "sit" }
}
```

Fields:
- `reminder_id`: the reminder affected.

### `seed.reminder.pinned` / `seed.reminder.unpinned`

A reminder was pinned to (or unpinned from) the orbit ring.

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.reminder.pinned",
  "data": { "reminder_id": "walk" }
}
```

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.reminder.unpinned",
  "data": { "reminder_id": "walk" }
}
```

Fields:
- `reminder_id`: the reminder affected. Pinned reminders appear in the orbit ring regardless of urgency.

### `seed.trait.xp_changed`

A trait's XP was adjusted (gain or drain). Emitted for XP drain during overdue windows as well as gains from completions.

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.trait.xp_changed",
  "data": {
    "delta": 55,
    "new_xp": 500,
    "trait_id": "core"
  }
}
```

Fields:
- `trait_id`: the affected trait.
- `delta`: XP change (positive = gain, negative = drain).
- `new_xp`: total XP for the trait after the change.

### `seed.trait.level_up`

A trait crossed a level boundary.

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.trait.level_up",
  "data": {
    "new_level": 5,
    "new_xp": 1154,
    "old_level": 4,
    "trait_id": "reach"
  }
}
```

Fields:
- `trait_id`: the trait that leveled up.
- `old_level` (`#[serde(default)]`): the level immediately before this event. Carried so the fold can compute `levels_gained = new_level - old_level` without reading pre-event state — `ReminderCompleted` may already have written `new_xp` into `state.traits` in the same commit batch. Default `0` lets old log entries deserialize cleanly; the fold then treats `old_level = 0` as a 1-level gain (safe undercount, not a loss).
- `new_level`: the level reached (1–99).
- `new_xp`: total XP at the moment of level-up.

Note: `seed.trait.level_up` is emitted alongside `seed.reminder.completed` when the same completion causes a level boundary to be crossed. Both appear in the same StateDiff batch. The fold uses `levels_gained` to advance `state.cumulative_levels_gained`, which drives focus token earning — see [`seed.focus.token_earned`](#seedfocustoken_earned).

### `seed.companion.tier_changed`

The companion advanced to a new tier. See [[tier-progression]] for the full ladder. Tiers are computed from total level (sum of all 9 trait levels). The progression is SEED → SPROUT → FROND → BLOOM → ORBIT → LATTICE → MANDALA → LUMEN → NEBULA → ZENITH (10 tiers; thresholds in `seed-core/src/domain.rs::Tier::min_total_level`).

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.companion.tier_changed",
  "data": {
    "from": "Seed",
    "to": "Sprout",
    "total_level": 20
  }
}
```

Fields:
- `from`: previous tier name. PascalCase enum variant (e.g. `"Seed"`, `"Lumen"`, `"Zenith"`).
- `to`: new tier name. PascalCase enum variant.
- `total_level`: sum of all 9 trait levels at the time of the change.

Tier thresholds (`min_total_level`):

| Tier | Threshold | Average level across 9 traits |
|---|---|---|
| `Seed` | 0 | 0 |
| `Sprout` | 18 | 2 |
| `Frond` | 63 | 7 |
| `Bloom` | 135 | 15 |
| `Orbit` | 270 | 30 |
| `Lattice` | 450 | 50 |
| `Mandala` | 630 | 70 |
| `Lumen` | 765 | 85 |
| `Nebula` | 855 | 95 |
| `Zenith` | 891 | 99 (all traits at 99) |

### `seed.companion.awakened`

The companion was first created, or was reset to initial state via the tweaks panel.

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.companion.awakened",
  "data": { "glyph_seed": 42 }
}
```

Fields:
- `glyph_seed`: the deterministic seed used to generate the mandala glyph. Constant per companion lifetime; changes on reset.

### `seed.config.changed`

A configuration value was changed at runtime (e.g. palette swap from the tweaks panel).

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.config.changed",
  "data": { "key": "palette", "value": "dusk" }
}
```

Fields:
- `key`: the configuration key changed (e.g. `"palette"`, `"snooze_min"`, `"notif_style"`).
- `value`: the new value (any JSON type — string for palette/notif_style, number for snooze_min).

### `seed.trait.integrated`

A trait was integrated: reset from level 99 to 1, with a visual enhancement appended to its glyph. Emitted at explicit user request. `apply_event` validates the trait is at level 99 and ignores the event defensively otherwise.

```json
{
  "v": 1,
  "ts": "2026-05-01T12:00:00Z",
  "kind": "seed.trait.integrated",
  "data": {
    "trait_id": "flow",
    "new_integrations": 1,
    "enhancement_id": "FlowSpiral"
  }
}
```

Fields:
- `trait_id`: the trait being integrated (must be at level 99).
- `new_integrations`: cumulative integration count for this trait after this event (1-based).
- `enhancement_id`: the visual enhancement chosen at this integration. Current values: `FlowSpiral`, `CoreEmber`, `SpineLattice`, `ReachBranch`, `ClarityRing`, `SpaceVeil`, `DepthAbyss`, `ResonanceChord`, `WarmthGlow`. Serialized as PascalCase string.

Effect: sets trait XP to 0 (level → 1), increments `trait_integrations[trait_id]`, appends `enhancement_id` to `trait_enhancements[trait_id]`.

### `seed.focus.token_earned`

A focus token was earned because cumulative trait levels gained crossed a 99-multiple boundary. Emitted from the daemon when it detects a level-up crosses the boundary; `cumulative_levels_gained` is updated deterministically in the `apply_event` fold for `LevelUp` so replays produce identical token totals.

```json
{
  "v": 1,
  "ts": "2026-05-01T12:00:00Z",
  "kind": "seed.focus.token_earned",
  "data": {
    "new_balance": 1
  }
}
```

Fields:
- `new_balance`: available token balance after this award (`cumulative_levels_gained / 99 - tokens_spent`).

Note: `apply_event` for this event is a no-op on state — `cumulative_levels_gained` is updated by the `LevelUp` fold. This event exists as a wire signal for UI notification.

### `seed.focus.phase_activated`

User spent a focus token to activate a phase. `apply_event` validates that `traits.len()` matches the pattern's expected count and that at least one token is available; ignores the event otherwise.

```json
{
  "v": 1,
  "ts": "2026-05-01T12:00:00Z",
  "kind": "seed.focus.phase_activated",
  "data": {
    "pattern": "Spread3x2",
    "traits": ["flow", "core", "spine"]
  }
}
```

Fields:
- `pattern`: one of `Concentrate1x4`, `Spread2x3`, `Spread3x2`. Determines the arrow count per trait (3/2/1) and the XP multiplier (4×/3×/2×).
- `traits`: trait IDs to allocate. Must contain exactly 1, 2, or 3 entries matching the pattern.

Effect: increments `tokens_spent`, replaces `active_focus` with a new `FocusPhase` using the specified pattern and arrow allocations. Prior active phase is discarded (no stacking). There is no `seed.focus.phase_ended` event — phases end only by being replaced.

### Unknown Events

Any event with an unrecognised `kind` round-trips through `Event::Unknown { kind, data }`. The `kind` string and full `data` payload are preserved verbatim. Consumers must tolerate unknown kinds for forward-compat.

Example (a hypothetical future event a v0 client would see as Unknown):

```json
{
  "v": 1,
  "ts": "2026-04-22T12:00:00Z",
  "kind": "seed.future.thing",
  "data": { "x": 42 }
}
```

## Consuming the Log

Simple tail (shell):

```bash
tail -f ~/.seed/events.jsonl | jq -r 'select(.kind == "seed.reminder.completed") | "\(.ts) \(.data.reminder_id)"'
```

Count completions by reminder:

```bash
jq -r 'select(.kind == "seed.reminder.completed") | .data.reminder_id' ~/.seed/events.jsonl | sort | uniq -c | sort -rn
```

For a Rust consumer, depend on `seed-core` and use `seed_core::events::{from_envelope, EventEnvelope}`:

```rust
use seed_core::events::{from_envelope, EventEnvelope};

for line in std::fs::read_to_string("~/.seed/events.jsonl")?.lines() {
    let env: EventEnvelope = serde_json::from_str(line)?;
    let event = from_envelope(env)?;
    // match on event variants...
}
```

## Versioning Policy

The full rules and rationale live at [[wire-versioning]]. Summary:

- __Adding a new `kind`__ does NOT bump `v`. Consumers must tolerate unknown kinds (they round-trip as `Unknown`).
- __Adding a new field to an existing kind's `data`__ does NOT bump `v`. Consumers must tolerate extra fields (serde ignores unknown fields by default).
- __Removing or renaming a field, changing a field's type, or renaming a kind__ DOES bump `v`. A daemon reading events with an unknown major `v` will log an error and skip the line rather than corrupt state.

The current version is `1`.
