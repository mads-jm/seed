---
date created: Monday, April 27th 2026, 9:00:00 am
date modified: Wednesday, April 29th 2026, 7:53:21 am
cssclasses: []
tags:
  - spec
  - prestige
  - focus
  - levels
status: implemented
---

# Prestige — Focus

> __Status__: fully implemented. Data model and event folds landed in Wave 6 ([[build-log-06-xp-calibration]]); UI surface (token chip, focus arrows, phase-chooser modal, toasts) shipped in Wave 7.

The second prestige system, parallel to [[prestige-integrate]]. A `tokens` currency awarded every +99 cumulative trait levels gained. Spending a token activates a "phase" in which the user allocates a 4× bonus budget across one or more traits.

## Why

Integrate is identity and visual reward — slow, lifetime-scoped, cosmetic. Focus is the opposite: tactical, mid-term, expressive in the *numbers*. It lets the user accelerate progress on whatever feels most important right now (e.g. lean into reflection traits during a hard week) without inflating the long-run pacing contract from [[xp-pacing]].

The two systems compose cleanly: integrate stacks visual layers, focus multiplies XP. They're independent enough that future tuning of one shouldn't disturb the other.

## Contract

### Token Earning

A token is awarded on every 99 cumulative trait levels gained over the companion's lifetime.

```
cumulative_levels_gained: u32   // sum of every level-up event ever observed
tokens_spent:             u32
tokens_available()        = (cumulative_levels_gained / 99) - tokens_spent
```

`cumulative_levels_gained` is __monotonic non-decreasing__. Integrate (which resets a trait's level to 1) does NOT subtract from it. The achievement of having reached those levels persists; the regrind counts forward, not backward.

__Prestige survives Reset.__ `Action::Reset` wipes per-trait XP and reminder runtime state but preserves all focus prestige state: `cumulative_levels_gained`, `tokens_spent`, `active_focus`, `trait_integrations`, and `trait_enhancements`. This is a load-bearing guarantee — prestige is a lifetime record of the user's investment, not a current-snapshot value. Resetting XP to start over does not erase that history.

### Phase Activation

Spending a token opens a "phase" in which the user picks one allocation pattern:

| Pattern | Skills affected | Multiplier per skill | Arrows shown |
|---------|-----------------|----------------------|--------------|
| `Concentrate1x4` | 1 | 4× | ▲▲▲ |
| `Spread2x3` | 2 | 3× each | ▲▲ on each |
| `Spread3x2` | 3 | 2× each | ▲ on each |

Patterns are not arithmetically equal — concentrate trades total power for peak XP rate; spread trades peak rate for breadth. This asymmetry is the design surface (the choice has weight). If the design later wants equal totals, swap `Concentrate1x4` for `Concentrate1x6` (one constant in the multiplier table).

Only __one focus configuration is active at a time__. Spending a new token replaces the prior allocation. There is no "save & continue" — phases end only by being replaced. There are no draft / preview states.

### Multiplier Application

Applied at XP-award time in `xp_reward`:

```rust
fn xp_reward(reminder: &Reminder, opts: XpRewardOpts, focus: Option<&FocusPhase>) -> u32
```

If `focus` has an allocation for the reminder's trait, multiply the result by `2 / 3 / 4` based on the arrow count for that trait. No focus, no multiplier. Initial state has `active_focus: None`, so behavior is unchanged until the user first spends a token.

### Pacing Implication

Focus deliberately breaks the [[xp-pacing]] contract for the duration of an active phase, but only on the allocated traits. The pacing band test in `crates/seed-core/tests/levels.rs` runs with `focus: None`, so the unboosted contract is what's enforced. A second test asserts that a `Spread3x2` allocation across 3 traits yields exactly 2× their daily-XP budget — locking the multiplier semantics.

## Data Model

### State Additions (companion-level)

```rust
struct State {
    // ...existing per-trait state, etc.
    cumulative_levels_gained: u32,
    tokens_spent: u32,
    active_focus: Option<FocusPhase>,
}

struct FocusPhase {
    pattern: FocusPattern,
    allocations: Vec<(TraitId, u8)>,  // (trait, arrow_count)
}

enum FocusPattern { Concentrate1x4, Spread2x3, Spread3x2 }
```

The `u8` in `allocations` is the arrow count (1, 2, or 3), which maps to multiplier (2×, 3×, 4×) via `arrow_to_multiplier()`. Storing the arrow count keeps the rendering code simple (just count the arrows to display) and the multiplier derivation explicit.

### Events

`Event::FocusTokenEarned { new_balance }` — `kind: "seed.focus.token_earned"`. Emitted from inside the `apply_event` fold for `LevelUp` whenever the increment to `cumulative_levels_gained` crosses a 99-multiple boundary. Computed deterministically so replays produce identical token totals — the [[event-sourcing]] + [[snapshot-and-replay]] invariants are load-bearing here.

`Event::FocusPhaseActivated { pattern, traits }` — `kind: "seed.focus.phase_activated"`. `traits: Vec<TraitId>` has exactly 1, 2, or 3 entries depending on `pattern`. `apply_event`:
1. Validate `traits.len()` matches the pattern (1 / 2 / 3 respectively); reject otherwise.
2. Validate `tokens_available() > 0`; reject otherwise.
3. Increment `tokens_spent`.
4. Replace `active_focus` with a new `FocusPhase` populated from the event.

There is no `Event::FocusPhaseEnded` — phases end only by being replaced.

Document both variants in [[events-schema]]. Both are additive ([[wire-versioning]]: new kinds, no `v` bump) so older clients tolerate them via `Event::Unknown` round-trip.

### Helpers

```rust
fn tokens_available(state: &State) -> u32 {
    state.cumulative_levels_gained / 99 - state.tokens_spent
}

fn can_activate_phase(state: &State) -> bool {
    tokens_available(state) > 0
}

fn arrow_to_multiplier(arrows: u8) -> u32 {
    match arrows { 1 => 2, 2 => 3, 3 => 4, _ => 1 }
}
```

## Implementation order

1. This spec.
2. State additions in `crates/seed-core/src/state.rs`.
3. `FocusPattern` enum + `FocusPhase` struct in `crates/seed-core/src/domain.rs`.
4. Update `xp_reward` signature to accept `Option<&FocusPhase>`; apply multiplier when allocation matches.
5. `Event::FocusTokenEarned`, `Event::FocusPhaseActivated` variants + folds in `crates/seed-core/src/events.rs`. Token-earning logic in the `LevelUp` fold.
6. Update daemon's `apply_event` for `ReminderCompleted` to read `state.active_focus` and pass it to `xp_reward`.
7. Document events in [[events-schema]].
8. Round-trip + fold tests in `crates/seed-core/tests/events.rs`. Replay determinism test for token-earning. Multiplier test in `crates/seed-core/tests/levels.rs`.

## User Interface

Shipped in Wave 7:

- __Token-balance chip__ (`★N`) in the title bar, shown only when `tokens_available > 0`.
- __Focus-token toast__ (`+1 focus token (N total)`) fired via StateDiff when `FocusTokenEarned` arrives.
- __Active focus arrows__ (`▲×N`) on LEVELS rows for traits with a current focus allocation.
- __`f` key__ — when command bar is empty and tokens > 0, opens the phase-chooser modal.
- __Phase-chooser modal__ — two-stage: Step 1 selects allocation pattern (Spread 3x2 / Spread 2x3 / Concentrate), Step 2 toggles traits (enforces arity per pattern), Enter dispatches `Action::ActivateFocusPhase`, Esc backs up or cancels.
- __`/focus <pattern> <trait1> [trait2] [trait3]`__ slash command — dispatches directly; validates pattern name and trait arity.

## Out of Scope (still deferred)

- Persistent focus history (timeline of past phases). Easy add later — events are already in the log.
- Interaction with [[prestige-integrate]] — they're independent; no combo bonus or cross-system effect.

## Risks

- The three patterns are not arithmetically equal (raw multiplier sums are 4 / 6 / 6). Read as deliberate concentrate-vs-spread tradeoff. If parity is desired, `Concentrate1x6` is a one-line change.
- A user could in principle hold many tokens and never spend them. That's fine — tokens persist. No expiry.
- Token-earning fires inside `LevelUp` fold, which means replaying an old event log produces the same token total deterministically. Care needed if `cumulative_levels_gained` initialization ever changes (e.g. retro-active count from existing snapshots) — bump events `v` in that case.

## See also

- [[xp-pacing]] — the unboosted contract focus multiplies against
- [[prestige-integrate]] — the visual / cosmetic prestige system
- [[event-sourcing]] — why token earning is part of the `LevelUp` fold rather than daemon state
- [[snapshot-and-replay]] — what makes token-total replay determinism load-bearing
- [[cli-flags]] — `--dev` mode is needed to manually emit focus events for testing
