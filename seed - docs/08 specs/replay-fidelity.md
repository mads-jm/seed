---
date created: Thursday, July 16th 2026, 12:00:00 pm
date modified: Thursday, July 16th 2026, 12:00:00 pm
cssclasses: []
tags:
  - spec
  - events
  - state
  - replay
  - snapshot
  - correctness
status: draft
---

# Replay Fidelity

Folding the event log must reproduce the state the daemon was actually holding. It currently does not. `CompanionAwakened` is a **no-op in `apply_event`**, while the `Reset` action that emits it rebuilds state in memory — so any refold that crosses a reset silently reconstructs a *different, wrong* state. `snapshot.json` is what hides this, which makes it load-bearing rather than derived, in direct contradiction of the architecture's central claim.

This is a correctness spec, not a feature. It has already caused real data loss (see [Evidence](#evidence)).

## Why

The architecture contract says state **is** the fold of an append-only log:

> `apply_event(state, event) -> state` is the single fold function; both daemon (writer) and TUI (reader, via StateDiff) use it.

[[CONCEPTS|The concepts index]] states the same rule more sharply, as the thing [[event-sourcing]] is supposed to guarantee:

> replay determinism. The single invariant that `apply_event` is the only state mutation, used by both daemon (writer) and TUI (reader).

`Action::Reset` violates that invariant literally: it mutates `State` directly and lets the fold see only a breadcrumb it then ignores. Two code paths end up disagreeing about what `CompanionAwakened` means.

`seed-daemon/src/daemon.rs:598` — `Action::Reset` rebuilds state **in memory**, carrying prestige forward, then commits `CompanionAwakened`:

```rust
let fresh = {
    let old = self.state.read().await;
    let mut s = seed_core::initial_state(now_ms);
    // Carry prestige fields forward.
    s.cumulative_levels_gained = old.cumulative_levels_gained;
    s.tokens_spent            = old.tokens_spent;
    s.active_focus            = old.active_focus.clone();
    s.trait_integrations      = old.trait_integrations.clone();
    s.trait_enhancements      = old.trait_enhancements.clone();
    s
};
```

`seed-core/src/events.rs:535` — the fold throws the event away:

```rust
Event::TierChanged { .. } | Event::CompanionAwakened { .. } | Event::Unknown { .. } => {}
```

So the reset lives **only** in the daemon's memory and in whatever snapshot it later writes. Replay never sees it. Every counter the reset was supposed to zero keeps accumulating across the whole log.

### Why It Stayed Hidden

Boot loads `snapshot.json` and replays only the *tail* past `events_consumed` (`seed-daemon/src/event_log.rs:130`). A refold across an old `CompanionAwakened` therefore never happens in normal operation — the snapshot carries the post-reset state and the tail is short.

The bug only surfaces on a **full refold**, i.e. exactly the thing the docs imply is always safe: delete the derived snapshot, let the log rebuild. That instinct is currently a data-loss operation.

## Evidence

Observed on a real `~/.seed` (2026-07-16), against a log whose last `CompanionAwakened` was 2026-07-10:

| Field | Live daemon / snapshot | Full refold | Truth |
|---|---|---|---|
| `completed_total` | 2 | **75** | 2 |
| `awakened_at` | `1783707110040` (Jul 10) | **boot time** | Jul 10 |
| activity `log` | 3 entries since reset | **all-time** | 3 |
| `cumulative_levels_gained` | 2453 | 2453 | 2453 |

75 is every `reminder.completed` in the log's entire history; 2 is the count since the reset. `cumulative_levels_gained` agrees **by coincidence** — it is preserved across `Reset` *and* accumulates in replay, so both paths land on the same number. That coincidence is why the divergence reads as "just one weird counter" instead of what it is.

`awakened_at` is the sharpest tell: on a full refold it becomes the **boot timestamp**, because `initial_state(now_ms)` is seeded at process start and nothing in the log ever moves it back.

Traits/XP survive intact — trait events carry absolute `new_xp`, so last-write-wins reconstructs them correctly regardless. The damage is confined to reset-scoped counters and lifecycle fields.

## Contract

`CompanionAwakened` means *"state was reset to `initial_state` here, prestige carried forward"*. The fold must say the same thing the daemon says:

```rust
Event::CompanionAwakened { glyph_seed } => {
    let mut s = initial_state(ts_ms);          // <- needs the event's timestamp
    s.cumulative_levels_gained = state.cumulative_levels_gained;
    s.tokens_spent             = state.tokens_spent;
    s.active_focus             = state.active_focus.clone();
    s.trait_integrations       = state.trait_integrations.clone();
    s.trait_enhancements       = state.trait_enhancements.clone();
    s.glyph_seed               = *glyph_seed;
    *state = s;
}
```

The preserved set must stay in lockstep with `daemon.rs`'s `Action::Reset`. Two lists that must agree and live 400 lines apart in different crates is the root cause here — whatever lands should make drift **fail a test**, not just be documented.

### The Timestamp Problem

`apply_event(state, event)` (`seed-core/src/events.rs:280`) has no timestamp, but `initial_state(now_ms)` (`seed-core/src/state.rs:161`) needs one for `awakened_at` / `last_tick_ms`. Options:

1. **Thread the envelope `ts`** — `apply_event(state, event, ts_ms)`, or an `apply_envelope(state, &EventEnvelope)` that unwraps it. The envelope already carries the authoritative `ts` on every event; nothing new is invented and historical logs replay correctly. Costs a signature change across the daemon fold, the TUI's StateDiff fold, and ~18 call sites in `seed-core/tests/events.rs`. **Preferred.**
2. **Add `at_ms` to the event** — `CompanionAwakened { glyph_seed, at_ms }`, matching the convention already used by `ReminderCompleted` / `ReminderNotified` / `ReminderSkipped`. Additive, so non-breaking per [[events-schema]]. But historical events lack the field and would need a `serde` default, which lands `awakened_at` on the epoch — i.e. it fixes new resets and leaves old logs wrong. Needs an envelope fallback anyway, at which point option 1 is simpler.

Note `TierChanged` shares the no-op arm. That one is arguably fine (it is derived/cosmetic, and per [[v0-1-0-punch-list#TASK-025 · Emit `TierChanged` + Tier-up Toast [L]]] it has zero producers), but it should be an explicit, commented decision rather than a shared shrug.

## Acceptance Criteria

- `apply_event` reconstructs the reset: folding a log containing `CompanionAwakened` yields the same state the daemon held after `Action::Reset`.
- A test folds `[…events…, CompanionAwakened, …events…]` from scratch and asserts the post-reset counters (`completed_total`, `missed_total`, per-trait XP, reminder runtimes) ignore everything before the reset, while the prestige set carries through.
- A test pins the preserved-field list against `daemon.rs`'s `Action::Reset` so the two cannot drift silently.
- `awakened_at` after a refold equals the `CompanionAwakened` envelope's `ts`, not the boot time. Regression target from the live log above: `1783707110040`.
- Deleting `snapshot.json` and restarting reproduces the pre-deletion state. That is the whole invariant, stated as a test.

## Out of Scope (Deferred)

- **Repairing already-diverged logs.** Any `~/.seed` that has been refolded across a reset now carries wrong counters. Once the fold is correct, a refold repairs it for free — no migration needed, so no migration should be written.
- **Making snapshots redundant.** Snapshots remain a boot-time optimisation. This spec only makes them *not load-bearing*.
- **The tick-vs-fold question generally.** Presence and other injected inputs are deliberately outside the fold ([[presence-grace]]); this spec does not relitigate that boundary. `CompanionAwakened` is different in kind — it is already an event, it is already in the log, and the fold simply ignores it.

## See Also

- [[event-sourcing]] — "the single invariant that `apply_event` is the only state mutation"; this spec is what makes that sentence true
- [[snapshot-and-replay]] — "why `apply_event` purity makes this safe". It is currently what makes the divergence *invisible*; that note should not be written until this is fixed, or it will document a guarantee that does not hold
- [[events-schema]] — the wire contract this fold implements
- [[presence-grace]] — the "injected, not folded" boundary, and why it is deliberate
- [[pure-core]] — why the fold takes `now` rather than reading a clock
- [[v0-1-0-punch-list#TASK-027 · `EventLog` Durability Fixes [H]]] — adjacent `EventLog` correctness work
