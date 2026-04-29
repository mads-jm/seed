---
date created: Monday, April 27th 2026, 9:00:00 am
date modified: Wednesday, April 29th 2026, 7:53:22 am
cssclasses: []
tags:
  - spec
  - prestige
  - integrate
  - levels
status: implemented
---

# Prestige — Integrate

> __Status__: fully implemented. Data model landed in Wave 6 ([[build-log-06-xp-calibration]]); UI surface (enhancement-chooser modal, `[I]` affordance, integration count badges) shipped in Wave 7.

The first of two prestige systems. Per-trait, voluntary, cosmetic-only. When a trait reaches lvl 99 the user may "integrate" it: the trait resets to 1 and the user picks a visual enhancement that persists on the glyph forever. Each subsequent integration of the same trait stacks another enhancement linearly.

## Why

The 1-year time-to-99 contract from [[xp-pacing]] makes lvl 99 a real achievement but not the end of the story. Integrate is the ritual that turns the achievement into something visible on the companion. It also keeps the gameplay loop alive past 99 without inflating XP rates — the reward is identity and visual depth, not faster numbers.

## Contract

### Trigger

Available only when the trait's level is exactly 99 (XP ≥ `xp_for_level(99)`). The user explicitly invokes integrate; nothing happens automatically at 99.

### Effect

1. Trait XP resets to 0 (level → 1).
2. The trait's `integrations: u8` counter increments.
3. The user's chosen enhancement (`IntegrationEnhancement`) is appended to that trait's `enhancements: Vec<IntegrationEnhancement>` list. Order is preserved; enhancements never replace each other.

### What Integrate Does NOT Do

- __No XP-rate change.__ Linear scaling means the *visual* effect stacks; XP/hr stays at the contract value. Pacing is preserved across integrations. If a future XP buff is wanted, model it as a separate `prestige_xp_mult` field — don't smuggle it into integrate.
- __No tier-table change.__ [[tier-progression]] is computed from total level across all 9 traits. Integrating one trait reduces total level by 98, so tier may step down — that's intentional. Tier is the snapshot; integrate is the lifetime achievement (tracked separately in the `integrations` counter and the persistent enhancements).
- __No cross-trait coupling.__ Each trait's integrations are independent. Integrating `flow` does not affect `core`.

## Data Model

### State Additions (per-trait)

```rust
struct TraitState {
    // ...existing xp, last_done_ms, etc.
    integrations: u8,                          // count of times integrated
    enhancements: Vec<IntegrationEnhancement>, // chosen visual enhancements, append-only
}
```

`u8` is sufficient — even at one integration per year over a lifetime you never overflow.

### `IntegrationEnhancement` Enum

A starter set is one cosmetic per trait (e.g. `FlowSpiral`, `CoreEmber`, `SpineLattice`, `ReachBranch`, `ClarityRing`, `SpaceVeil`, `DepthAbyss`, `ResonanceChord`, `WarmthGlow`). Expandable; serialized by stable string id so the catalog can grow without breaking persisted state.

Selection happens at integration time. The user-facing UI (deferred) presents available enhancements; this spec only commits to the data model.

### Event

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

`apply_event` for `TraitIntegrated` ([[event-sourcing]] fold; lives in [[pure-core]]):

1. Validate trait is at lvl 99 (else ignore — defensive against malformed event injection).
2. Set `xp = 0`.
3. Increment `integrations`.
4. Push `enhancement_id` onto `enhancements`.

Document this variant in [[events-schema]]. Adding it is non-breaking ([[wire-versioning]]: new kind, no `v` bump).

## Glyph Rendering

Out of scope for the data-model pass. When rendering ships:

- Each enhancement maps to a renderer hook that draws additional layers / modifies char distribution / shifts color blend for that trait's contribution to the glyph. See [[glyph-layer-composition]].
- Multiple enhancements on the same trait stack additively (linear scaling — three FlowSpirals are three nested spirals, not one larger spiral).
- Renderer remains pure / deterministic from `(traits, enhancements, seed)` — same [[pure-core]] invariant as the rest of the rendering pipeline.

## Implementation order

1. This spec.
2. State additions in `crates/seed-core/src/state.rs`.
3. `IntegrationEnhancement` enum in `crates/seed-core/src/domain.rs`.
4. `Event::TraitIntegrated` variant + fold in `crates/seed-core/src/events.rs`.
5. Document the event in [[events-schema]].
6. Round-trip + fold tests in `crates/seed-core/tests/events.rs` — confirm xp resets, integrations increments, enhancement persists across snapshot save/load.

## User Interface

Shipped in Wave 7:

- __`[I]` affordance__ on LEVELS rows when trait XP ≥ `xp_for_level(99)` — signals integrate is available.
- __Integration count badge__ (`✦N`) on LEVELS rows and in the skill-detail overlay when `N > 0`.
- __Enhancement-chooser modal__ — opens on Enter at a level-99 LEVELS row (or via `/integrate <trait>`). Shows prior integration count, one enhancement option per trait (starter set), Enter to confirm, Esc to cancel.
- __`/integrate <trait> [enhancement]`__ slash command — dispatches `Action::Integrate` directly; enhancement defaults to the per-trait starter if omitted.

## Out of Scope (still deferred)

- Glyph rendering of enhancements.
- Enhancement catalog beyond the per-trait starter set.
- Cross-trait combo enhancements ("when both flow and space are integrated, unlock X") — tempting but out of scope until the base system has lived experience behind it.

## See also

- [[xp-pacing]] — the contract integrate must preserve
- [[prestige-focus]] — the parallel prestige system (currency-driven, XP-multiplier-based)
- [[event-sourcing]] — why this is just one more event variant against the existing fold
- [[glyph-layer-composition]] — the substrate enhancements stack on
- [[tier-progression]] — why integrating one trait can step the companion's tier down
