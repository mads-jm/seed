---
date created: Monday, April 27th 2026, 12:00:00 pm
date modified: Wednesday, April 29th 2026, 7:53:22 am
cssclasses: []
tags:
  - note
  - build-log
  - wave-6
  - xp
  - prestige
status: archived
---

# Build Log 06 — XP Calibration + Prestige Pre-wiring

__Date__: 2026-04-27 · __Wave__: 6 · __Specs__: [[xp-pacing]] · [[prestige-integrate]] · [[prestige-focus]]

## Scope

Reshape the XP economy to enforce the 1-year-to-99 contract uniformly across all 9 traits, and pre-wire the two prestige systems' data models without UI surface. Eight stages, all shipped.

## What Shipped

| Stage | What |
|---|---|
| __A__ | `SCALE_DIVISOR = 10` in `levels.rs`. `xp_for_level(99) → 1,303,443`. Lvl 92 / lvl 99 ratio ≈ 0.5007 (within 0.02 epsilon). |
| __B__ | `xp_per_completion: u32` field on `Reminder`. All 20 catalog entries populated from the [[xp-pacing]] table. Timing multipliers tightened to `0.6 / 1.0 / 1.4`. |
| __C__ | `xp_reward(reminder, opts, focus: Option<&FocusPhase>) -> u32`. Old `xp_reward(opts)` signature removed. |
| __D__ | `IntegrationEnhancement` enum (9 variants). `trait_integrations: BTreeMap<TraitId, u8>` and `trait_enhancements: BTreeMap<TraitId, Vec<IntegrationEnhancement>>` on `State`. `Event::TraitIntegrated` + fold. |
| __E__ | `FocusPattern` + `FocusPhase` in `domain.rs`. `cumulative_levels_gained`, `tokens_spent`, `active_focus` on `State`. `Event::FocusTokenEarned` + `Event::FocusPhaseActivated` + folds. Helpers: `tokens_available()`, `can_activate_phase()`, `arrow_to_multiplier()`. |
| __F__ | Daemon `build_complete_events` reads `state.active_focus`, passes to `xp_reward`, detects 99-multiple boundary and emits `FocusTokenEarned` before dropping the read lock. |
| __G__ | Pacing band test (per-trait), focus multiplier tests, round-trip tests for all 3 new events, fold tests, replay determinism test, snapshot persistence test, old-snapshot backward-compat test. |
| __H__ | Wire schema doc updated with the 3 new event variants. |

After this wave: 163/163 tests in `seed-core`; 36/36 in `seed-tui` lib. Clippy + fmt clean.

## Technical Decisions

- __`LevelUp` fold can't return derivative events__ — `apply_event` is `&mut State → ()` (the [[event-sourcing]] / [[pure-core]] contract). We need to emit `FocusTokenEarned` deterministically when `cumulative_levels_gained` crosses a 99-multiple. __Solution__: the fold mutates `cumulative_levels_gained` (deterministic on replay); the daemon's `build_complete_events` detects the boundary crossing pre/post-fold and emits the wire signal. `FocusTokenEarned`'s own fold is a no-op — it's a UI signal, like `CompanionAwakened` and `TierChanged`. The [[snapshot-and-replay]] invariant holds because the cumulative state is what carries forward, not the signal event.

- __`xp_reward` composition order__ — `xp_per_completion × time_mult × focus_mult`, left to right, fp throughout, rounded at the end. The on-time base is canonical; both multipliers fan around it. Documented in the function comment.

- __Pacing band wide ceiling for `clarity` / `space` / `reach`__ — one ultra-frequent reminder (`look` 20 min, `breathe` 25 min) paired with one anchor reminder (`sun` / `rest` 240 min) sums above 4200/day; the per-reminder XP/hr is within contract, only the trait sum is high. `reach` compounds three moderate-cadence reminders. Each individually on-band. The wide ceiling (6000) is not slack — a 15% drift falls outside.

- __`depth` fire-rate calculation__ — reminders with interval > active window (`journal`, `reflect` at 1440 min vs. 900 min active) fire at `1/day`, not `900/1440 = 0.625/day`. The pacing test now branches on `interval > active_window`.

- __OSRS ratio epsilon widened to 0.02__ — floor division after `SCALE_DIVISOR = 10` accumulates rounding. The actual ratio is 0.5007 (well within 0.01), but the spec asked for 0.02 with a comment.

- __No `tracing` in `seed-core`__ — the fold needs to validate inputs but `seed-core` has no `tracing` dep ([[pure-core]]). Used `#[cfg(debug_assertions)] eprintln!` instead. Tracked in [[v0-1-0-punch-list#Minor / nit cleanup batch|the nit batch]] — `eprintln!` from a fold breaks raw-mode TUI rendering and should move to the daemon.

## Deferred to V0.2.0

- Integrate UI: enhancement chooser at lvl 99.
- Focus UI: token balance display, phase chooser, arrow rendering on LEVELS.
- Glyph rendering of enhancements (each enhancement adds a layer to the trait's contribution).
- A minimum-viable hint surface in v0.1.0 — token counter on LEVELS or "INTEGRATE READY" status — is tracked as TASK-024 in [[v0-1-0-punch-list]].

## See also

- [[build-log-06b-redline-fixes]] — inspector findings on this wave + corrective patches.
- [[xp-pacing]], [[prestige-integrate]], [[prestige-focus]] — the specs.
- [[v0-mvp]] — consolidated milestone.
- [[event-sourcing]], [[pure-core]], [[snapshot-and-replay]] — the invariants the prestige pre-wiring leans on.
