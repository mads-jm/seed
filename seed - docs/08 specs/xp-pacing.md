---
date created: Monday, April 27th 2026, 9:00:00 am
date modified: Wednesday, April 29th 2026, 7:53:26 am
cssclasses: []
tags:
  - spec
  - xp
  - levels
status: implemented
---

# XP Pacing Contract

The XP curve and per-reminder reward values that determine how long it takes to reach lvl 99 in each trait. This is the source of truth: changes to reminder cadences or XP rewards must keep the contract intact, enforced by the pacing band tests in `crates/seed-core/tests/levels.rs`.

## Problem

`seed` ports the OSRS XP curve verbatim (lvl 99 = 13,034,431 XP, lvl 92 ≈ half) but pairs it with a single global reward formula — `base=55` × 0.55 / 1.35 / 2.0 for late / on-time / overdue — regardless of which reminder fired or which trait it feeds. The reminder cadences then implicitly determine each trait's XP/day, with extreme variance.

Under perfect adherence (15 active hours/day, 74 XP per on-time completion):

| trait | reminders (interval_min) | fires/day | XP/day | days to 99 | years |
|-------|--------------------------|-----------|--------|------------|-------|
| clarity | look (20), sun (240) | ~49 | ~3,600 | ~3,600 | ~10 |
| spine | stand (50), align (30) | ~48 | ~3,550 | ~3,670 | ~10 |
| space | breathe (25), rest (240) | ~40 | ~2,940 | ~4,430 | ~12 |
| reach | walk (90), stretch (60), shake (120) | ~33 | ~2,400 | ~5,420 | ~15 |
| flow | water (45), steep (180) | ~25 | ~1,850 | ~7,050 | ~19 |
| core | eat (180), graze (120) | ~12 | ~890 | ~14,650 | ~40 |
| resonance | sit (360), read (480) | ~4.4 | ~324 | ~40,200 | ~110 |
| warmth | tidy (300), reach (720) | ~4.3 | ~314 | ~41,500 | ~114 |
| depth | journal (1d), reflect (1d), thanks (12h) | ~4 | ~296 | ~44,000 | ~120 |

12× spread between fastest and slowest, even the fastest ~10 years. The OSRS curve is decorative — pacing is emergent accident.

In OSRS, every training method has a deliberately balanced XP/hr (fishing 30–70k, slayer 30–100k, combat 80–500k) and lvl 99 is calibrated to ~200–400 hours of dedicated grinding. The curve only makes sense when paired with a per-action XP/hr contract. seed needs the same discipline.

## Contract

### Target

Lvl 99 = ~365 days at perfect adherence, __uniform across all 9 traits__. Depth and flow both pace to the same time-to-99; reflective traits earn larger rewards per (rarer) action to balance the budget.

### Curve Scale

The OSRS table is rescaled by `SCALE_DIVISOR = 10`. Lvl 99 = __1,303,443 XP__ (was 13,034,431). The shape is preserved: lvl 92 ≈ half of lvl 99, exponential top-end. Absolute XP values become legible 2–4 digit numbers, not 4–7 digit ones.

### Per-trait Daily Budget

```
1,303,443 / 365 ≈ 3,571 XP/day
active_hours    = 15 hr/day → 238 XP/hr/trait (central target)
central band    = 200–280 XP/hr (±15% around 238)
```

A single ±15% band cannot hold all 9 traits because reminder cadences vary structurally. A single narrow band that passes `flow` (two intra-window reminders at moderate cadence) would reject `clarity` (one very-frequent 20-min reminder + one 4-hour anchor), even though each individual `clarity` reminder is within contract when measured per-reminder. The pacing test therefore uses __per-trait bands__ derived from each trait's fire profile.

### Per-trait Pacing Bands (XP/day)

| trait | band lo | band hi | rationale |
|-------|---------|---------|-----------|
| flow | 2700 | 3800 | 2 intra-window reminders; tight band |
| core | 2700 | 3800 | 2 moderate-cadence reminders |
| spine | 2700 | 3800 | 2 moderate-cadence reminders |
| depth | 2700 | 4200 | 3 reminders incl. 24h-cycle anchors |
| resonance | 2700 | 4500 | 2 long-interval reminders; rounding spread |
| warmth | 2700 | 4000 | 2 low-cadence reminders |
| reach | 3500 | 5500 | 3 reminders compound; each individually on-band |
| clarity | 3000 | 6000 | `look` (20 min) + `sun` (4 hr) extreme cadence spread |
| space | 3000 | 6000 | `breathe` (25 min) + `rest` (4 hr) extreme cadence spread |

__clarity__ and __space__ band rationale: their 4-hour anchor reminders (`sun`, `rest`) pay ~720 XP per fire to hit the XP/hr contract. At 3.75 fires/day each anchor reminder contributes ~2,700 XP/day on its own; the high-frequency companion (`look` at 45 fires/day × 60 XP = 2,700 XP/day) doubles the total. The wider ceiling is not "loose" — a 15% cadence drift in either reminder would fall outside even this wider band.

The band floors (2700–3000) correspond to roughly 180–200 XP/hr — slightly below the ±15% floor to account for the lowest-cadence reminders occasionally missing active-hour windows without breaking the contract.

The band is enforced by `pacing_band_per_trait` in `crates/seed-core/tests/levels.rs`. A reminder cadence change that pushes its trait outside its band fails CI.

### Late / Overdue Multipliers

Tightened from `0.55 / 1.35 / 2.0` to __`0.6 / 1.0 / 1.4`__. The previous 2× overdue bonus rewarded tardiness; the new schedule makes punctuality (1.0) the canonical reward and fans the multipliers symmetrically around it. `overdue` retains a small comeback bonus; `late` has a soft penalty.

## Per-reminder XP Rewards

`xp_per_completion` is baked onto each `Reminder` in the static catalog (`crates/seed-core/src/domain.rs`). Derived once via:

```
xp_per_completion = round(daily_budget / reminders_per_day_for_this_trait)
```

so each reminder contributes equal XP/hr to its trait, and per-trait totals sum to the daily budget.

| reminder | trait | fires/day | xp_per_completion |
|----------|-------|-----------|-------------------|
| water | flow | 20 | 145 |
| steep | flow | 5 | 145 |
| eat | core | 5 | 320 |
| graze | core | 7.5 | 285 |
| stand | spine | 18 | 95 |
| align | spine | 30 | 60 |
| walk | reach | 10 | 165 |
| stretch | reach | 15 | 110 |
| shake | reach | 7.5 | 220 |
| look | clarity | 45 | 60 |
| sun | clarity | 3.75 | 720 |
| breathe | space | 36 | 75 |
| rest | space | 3.75 | 715 |
| journal | depth | 1 | 1,200 |
| reflect | depth | 1 | 1,200 |
| thanks | depth | 2 | 600 |
| sit | resonance | 2.5 | 815 |
| read | resonance | 1.875 | 1,090 |
| tidy | warmth | 3 | 600 |
| reach | warmth | 1.25 | 1,400 |

The `reach` verb is reused — it doubles as both the trait id (`reach`, the movement trait) and a reminder word under the `warmth` trait (reach out to a friend). Catalog disambiguates by `(reminder_id, cat, trait_id)`; the parser routes the verb against `REMINDERS.word`.

High-effort, low-cadence reminders (journaling, reading, reaching out) carry the chunky rewards. This feels right narratively — sitting with morning pages for 20 minutes is a heavier act than a sip of water — and the daily totals balance.

## Implementation order

1. This spec.
2. Rescale OSRS table — add `SCALE_DIVISOR` constant in `crates/seed-core/src/levels.rs`. Update `level_99_canonical` test to assert 1,303,443. Other ratio-based tests pass unchanged.
3. Add `xp_per_completion: u32` field on `Reminder`; populate all 20 entries from the table above.
4. Replace the global base in `xp_reward` with a per-reminder lookup: `xp_reward(reminder: &Reminder, opts) -> u32`. Tighten multipliers to `0.6 / 1.0 / 1.4`.
5. Add pacing band test in `crates/seed-core/tests/levels.rs`: for each trait, sum `(reminders_per_day × xp_per_completion)` across its reminders, assert ∈ `200×15 .. 280×15` XP/day window.
6. Update all `xp_reward` callers (handful in daemon and TUI) to the new signature.

## Risks

- The table assumes perfect adherence. Real users skip and snooze; effective time-to-99 will stretch. The contract is a *ceiling*, not the median experience.
- Tightened multipliers (0.6 / 1.0 / 1.4) change vibe — overdue used to feel like a comeback bonus; now it's softer. If the comeback feel matters, swap to 0.6 / 1.0 / 1.2 and lean on streaks elsewhere. One constant.
- `sun` and `rest` (4hr cadence) pay ~720 XP per fire. That's a big toast magnitude. Consider whether daily-anchored reminders deserve distinctive toast styling so the number reads as intentional.

## Out of Scope

- Streak bonuses, time-of-day modifiers, anchor-hour bonuses (focus prestige is the only multiplier wired — see [[prestige-focus]]).
- Asymmetric per-trait pacing. Uniform 1-year is the contract; revisit only after lived experience.
- Tier-table changes — [[tier-progression]] is in level space and survives the rescale untouched.
- Reminder cadences themselves — they're the input to this contract, not the output. Cadence changes flow through [[reminder-lifecycle]] and the per-trait pacing band test catches drift.

## See also

- [[prestige-integrate]] — per-trait reset to 1 with persistent visual enhancement
- [[prestige-focus]] — `tokens` currency that activates a 4× bonus phase
- [[reminder-lifecycle]] — the state machine that determines on-time / late / overdue
- [[tier-progression]] — total-level → tier mapping, unaffected by the curve rescale
- [[cli-flags]] — daemon and TUI flags relevant to dev/test of XP behavior
