---
date created: Monday, July 13th 2026, 12:00:00 pm
date modified: Monday, July 13th 2026, 12:00:00 pm
cssclasses: []
tags:
  - spec
  - scheduling
  - reminders
  - presence
  - grace
status: draft
---

# Presence Grace

Stop punishing absence. When the user is away from the machine, missed cycles roll forward **without penalty** — no streak break, no `total_missed`, no per-trait skip aggregation, no XP drain — while the board still returns to a clean "upcoming" state on return. Introduces a **presence signal** injected into the scheduler tick (like `now`), a freshness TTL, and a penalty-free variant of the existing skip rollover.

This is the "catch-up" half of the [[seed-v02-direction|v0.2.0 direction]]: grace is the *felt* fix that ships before any XP-curve change. The deeper reframe — pacing XP to configured/present active time — is [Out of scope](#out-of-scope-deferred) here and lands with the [[xp-pacing]] rework once the [configurable active window](#see-also) is real.

## Why

[[overdue-rollover]] deliberately *penalizes* absence. Past `2×interval` the scheduler emits `ReminderSkipped` → streak reset + `total_missed++` + `traits_skipped` aggregation, and while `Overdue` inside the active window it drains XP. The active-hours gate stops *overnight* bleed, but it does nothing for **daytime absence**: step away for a hike at noon and the companion wilts and skips accrue against you. The lived complaint: *"I'm punished for being off my computer on a beautiful day."*

Recurring reminders are habits, not obligations you owe the machine. Time genuinely away should be **invisible**, not penalized — the board should still be clean when you return, but nothing should have counted against you.

Critically, the daemon already has a presence signal available that most apps don't: it is wired through to the OS lock screen and status bar. Grace consumes that signal.

## Contract

### Presence Is Injected, Not Folded

Presence is **environmental input**, exactly like the wall clock. It is *not* part of the event-log fold. The scheduler tick gains one parameter:

```rust
pub async fn tick(
    state: &mut State,
    config: &Config,
    notifier: &dyn Notifier,
    is_present: bool,   // NEW — injected by the daemon, like `now`
    now: DateTime<Utc>,
) -> Vec<Event>
```

Why injected rather than event-sourced: presence gates *which events get produced* at tick time; it never changes how an event *folds*. The events a tick emits (`TraitXpChanged`, `ReminderSkipped`) are still logged and replay identically. Keeping presence out of the fold preserves the [[pure-core]] and [[snapshot-and-replay]] invariants — replay of a log reproduces state regardless of the presence history at the time.

### The Presence Signal

The daemon maintains presence in its **own runtime** (not `State`):

```rust
struct PresenceTracker {
    last_active_ms: Option<i64>,   // last heartbeat / edge; None until first signal
    active: bool,                  // last explicit edge (unlock=true, lock=false)
}
```

Fed by a new IPC action pushed from external integrations (lock screen, status bar):

```rust
Action::SetPresence { active: bool }
```

- **Unlock / user-active edge** → `SetPresence { active: true }`; bumps `last_active_ms = now`, `active = true`.
- **Lock / idle edge** → `SetPresence { active: false }`; `active = false`.
- A status-bar **heartbeat** may send `SetPresence { active: true }` periodically to refresh liveness.
- Any *other* inbound `Action` (Complete, Snooze, …) also bumps `last_active_ms` — interacting with seed is proof of presence.

`is_present` is computed at tick time:

```
PRESENCE_TTL = 5 min      // heartbeat interval + margin

is_present =
  match last_active_ms {
    None        => in_active_hours,                 // no integration wired → today's behavior
    Some(t)     => active && (now_ms - t < PRESENCE_TTL_MS)
  }
```

**Backward-compatible default:** until a presence signal ever arrives, `is_present` falls back to `in_active_hours` — identical to current behavior. Grace is *opt-in by wiring the signal*. Once signals start arriving, the heartbeat-with-TTL rules take over. A stale heartbeat (integration crashed) decays to "absent" after `PRESENCE_TTL`, which fails **lenient** (no drain, penalty-free skips) — the intended bias.

### Grace Semantics In The Tick

The `ReminderState::Overdue` arm changes in two places; everything else is unchanged.

**1. Rollover still fires, but penalty-free when absent.** The board must still clean up (a wall of `OVRD` on return is the opposite of grace), so the `> 2×I` skip still emits. When `!is_present`, it is marked penalty-free:

```rust
if overdue_ms > interval_ms {
    let missed = (overdue_ms / interval_ms) as u32;
    let was_snoozed = rt.snoozed_until_ms > rt.last_done_ms;
    events.push(Event::ReminderSkipped {
        at_ms: now_ms,
        missed_cycles: missed,
        reminder_id: rid.clone(),
        was_snoozed,
        during_grace: !is_present,   // NEW
    });
    continue;
}
```

**2. Drain gates on presence, not just active hours.** This is the core "beautiful day" fix:

```rust
// was: if !in_active_hours { continue; }
if !is_present || !in_active_hours { continue; }
```

`is_present` already subsumes the overnight case (asleep → no heartbeat → absent), but active-hours is retained as an independent necessary condition: even if a heartbeat says you're at the machine at 02:00, a `07–22` window means no drain.

### apply_event Semantics

`during_grace` composes with the existing `was_snoozed` leniency. The penalty applies **only when the skip was neither snoozed nor during grace**:

```
rt.last_done_ms = at_ms                             // ALWAYS — board cleanup is unconditional
let penalize = !was_snoozed && !during_grace;
if penalize {
    rt.streak = 0;
    rt.total_missed += 1;                           // saturating
    state.traits_skipped[trait].lifetime += 1;      // saturating
    state.traits_skipped[trait].recent.push(at_ms); // pruned to 7d
}
```

So a grace skip: rolls the reminder forward to `Dormant`, preserves streak, does not count as missed, does not colour the LEVELS `▾N` indicator. It is a silent, clean reset.

### Notifications Are Unchanged

Grace is about *penalty*, not *nagging*. The `Due` notification path keeps its existing active-hours + snooze gating. Whether a *locked screen* should also suppress the OS notification is a separate question left to the integration layer (the lock screen owns its own do-not-disturb). Not specified here.

## Data Model

### Event Field (additive)

```rust
Event::ReminderSkipped {
    at_ms: i64,
    missed_cycles: u32,
    reminder_id: ReminderId,
    #[serde(default)]
    was_snoozed: bool,
    #[serde(default)]      // NEW — old log entries deserialize as false
    during_grace: bool,
}
```

Adding a field is non-breaking per [[wire-versioning]]; older readers preserve it via `Event::Unknown` round-trip, and `#[serde(default)]` lets pre-grace logs replay as `during_grace = false`. Documented in [[events-schema]].

### New Action

```rust
Action::SetPresence { active: bool }
```

Handled in `crates/seed-daemon/src/daemon.rs`: updates `PresenceTracker`, returns `Ok`. Emits an observational `Event::PresenceChanged` **only on edge transitions** (not per heartbeat — that would flood the log).

### Observational Event (edge-only)

```rust
Event::PresenceChanged { active: bool, at_ms: i64 }   // kind: seed.presence.changed
```

`apply_event` mirrors it to a `state.present: bool` field (`#[serde(default = true)]`) used **only** for the TUI to render a present/away dot and a LOG line. It does **not** feed the scheduler — the injected `is_present` runtime value is the sole gating input. This split keeps the log observable without letting an observational mirror drift into domain logic.

### CLI Surface

The same `seed` binary that gains `seed log <verb>` (TASK-014) gains:

```
seed presence active     # → Action::SetPresence { active: true }
seed presence idle       # → Action::SetPresence { active: false }
```

so the lock screen (`unlock → seed presence active`, `lock → seed presence idle`) and status bar (periodic `seed presence active`) wire in with one-line hooks. This depends on the TASK-014 CLI→daemon plumbing landing first.

## Implementation order

1. This spec.
2. Add `during_grace: bool` (`#[serde(default)]`) to `Event::ReminderSkipped`; update the `apply_event` arm so the penalty branch is gated by `!was_snoozed && !during_grace`. Round-trip + byte-snapshot tests for the new field.
3. Add `Event::PresenceChanged` + `state.present` mirror; `is_known_kind` / `event_kind` / `apply_event` arms (mirror only). Document both kinds in [[events-schema]].
4. Add `Action::SetPresence { active }` to `wire.rs`; handler in `daemon.rs` updating a `PresenceTracker`; emit `PresenceChanged` on edges only.
5. Thread `is_present` into `schedule::tick`; compute it in the daemon loop from `PresenceTracker` + `now`. Change the drain gate to `!is_present || !in_active_hours`; set `during_grace: !is_present` on the rollover skip.
6. `seed presence active|idle` CLI subcommand (rides TASK-014's client plumbing).
7. Tests in `crates/seed-daemon/src/schedule.rs`:
   - `overdue_while_absent_emits_grace_skip_no_drain` (present=false, daytime, overdue < 2×I → no drain, no skip; > 2×I → skip with `during_grace=true`, no `TraitXpChanged`).
   - `overdue_while_present_still_drains` (present=true, active hours, < 2×I → drains, as today).
   - `grace_skip_preserves_streak_and_missed` (apply-side: `during_grace=true` leaves streak/`total_missed`/`traits_skipped` untouched).
   - `no_presence_signal_falls_back_to_active_hours` (`last_active_ms=None` → behaves exactly as pre-grace).
   - `stale_heartbeat_decays_to_absent` (last heartbeat > TTL → treated absent).

## Risks

- **Default-lenient decay.** If a user wires the signal but the integration stalls, presence decays to absent and drain silently stops. Chosen deliberately (grace bias), but it means "companion never wilts" can mask a broken hook. Mitigation: the TUI present/away dot (`state.present`) makes the state visible; a stuck-idle dot is the tell.
- **`during_grace` vs `was_snoozed` proliferation.** Two boolean leniency flags now gate the same penalty branch. [[overdue-rollover]] already anticipates a `SkipKind`/`SkipReason` consolidation for manual skip; when that lands, fold `was_snoozed` + `during_grace` + manual into one `reason` enum. Not now — additive booleans keep this change small and replay-safe.
- **Presence granularity is coarse.** One global `is_present` gates all reminders. That's correct for "away from machine," but a future per-reminder or per-context presence (e.g. "in a meeting") is a different model. Out of scope.
- **Interaction with the pacing contract.** Grace makes effective XP/day under partial adherence rise (fewer drains, penalty-free skips) but never touches the on-time budget the [[xp-pacing]] band test models, so CI is unaffected. The 1-year-to-99 *ceiling* stays correct; the median moves closer to it — the intended effect.

## Out of Scope (deferred)

- **Presence-gated pacing.** Scaling the daily XP budget to *configured/present* active time (a genuine 8-hour day paces to 8 hours, not a hardcoded 15) is the structural half of catch-up. It touches [[xp-pacing]] and its band tests and depends on the [configurable per-day active blocks](#see-also) existing first. Tracked in [[seed-v02-direction]]; specced separately with the curve rework.
- **Daemon self-detected idle.** Having `seedd` probe OS idle time directly (X11/Wayland) instead of receiving a pushed signal. Wayland idle is fiddly and headless; the pushed-signal model is simpler and already matches the wired setup. Revisit only if the push integration proves unreliable.
- **Locked-screen notification suppression.** Whether a locked session should also mute the OS notification (vs. only suppressing penalty). Left to the integration's own DND for now.
- **Grace as a completion catch-up.** A "you were away — claim N completions" bankable reward. Explicitly rejected in favour of *invisible* absence; revisit only if lived experience wants a comeback moment.

## See also

- [[overdue-rollover]] — the rollover this spec makes penalty-free when absent; shares the `apply_event` skip arm and the `was_snoozed` leniency machinery
- [[xp-pacing]] — the on-time budget grace leaves untouched; home of the deferred presence-gated pacing rework
- [[reminder-lifecycle]] — the `Off/Dormant/Due/Overdue` machine; grace adds no new state, only a penalty-free transition
- [[event-sourcing]] / [[pure-core]] — why presence is injected like `now` rather than folded
- [[events-schema]] — where `seed.reminder.skipped` (new `during_grace`) and `seed.presence.changed` are documented
- [[cli-flags]] — `seed presence active|idle` subcommand and the daemon-side action
