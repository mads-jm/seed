use seed_core::{
    MAX_LEVEL,
    domain::{FocusPattern, FocusPhase, REMINDERS, TraitId, category_by_id},
    level_for_xp, level_norm, level_progress,
    levels::{XpRewardOpts, arrow_to_multiplier, xp_reward},
    xp_for_level, xp_to_next,
};

#[test]
fn level_99_canonical_xp() {
    // Rescaled by SCALE_DIVISOR=10: 13,034,431 / 10 = 1,303,443 (floor).
    assert_eq!(xp_for_level(99), 1_303_443);
}

#[test]
fn level_92_is_approximately_half_of_99() {
    let xp92 = xp_for_level(92) as f64;
    let xp99 = xp_for_level(99) as f64;
    let ratio = xp92 / xp99;
    // Actual ratio ≈ 0.5007 after SCALE_DIVISOR=10 floor division.
    // 0.01 epsilon is sufficient and meaningful (0.5007 - 0.5 = 0.0007 < 0.01).
    assert!(
        (ratio - 0.5).abs() < 0.01,
        "xp_for_level(92)/xp_for_level(99) = {ratio:.4} — expected within 1% of 0.5"
    );
}

#[test]
fn level_for_xp_round_trip() {
    for n in 1u8..=MAX_LEVEL {
        let xp = xp_for_level(n);
        assert_eq!(
            level_for_xp(xp),
            n,
            "round-trip failed at level {n}: xp_for_level({n})={xp}, level_for_xp({xp})≠{n}"
        );
    }
}

#[test]
fn xp_for_level_1_is_zero() {
    assert_eq!(xp_for_level(1), 0);
}

#[test]
fn xp_for_level_0_clamps_to_zero() {
    // level 0 is below minimum; function treats ≤1 as 0
    assert_eq!(xp_for_level(0), 0);
}

#[test]
fn level_for_xp_zero_is_one() {
    assert_eq!(level_for_xp(0), 1);
}

#[test]
fn xp_to_next_at_max_level_is_zero() {
    assert_eq!(xp_to_next(xp_for_level(MAX_LEVEL)), 0);
}

#[test]
fn level_progress_at_max_is_one() {
    assert_eq!(level_progress(xp_for_level(MAX_LEVEL)), 1.0);
}

#[test]
fn level_norm_at_max_is_one() {
    let n = level_norm(xp_for_level(MAX_LEVEL));
    assert!((n - 1.0).abs() < f32::EPSILON, "level_norm at max = {n}");
}

#[test]
fn level_progress_midway() {
    // Halfway between level 10 and level 11
    let base = xp_for_level(10);
    let next = xp_for_level(11);
    let mid = base + (next - base) / 2;
    let p = level_progress(mid);
    assert!((p - 0.5).abs() < 0.01, "expected progress ~0.5, got {p}");
}

#[test]
fn levels_are_monotonically_increasing() {
    let mut prev = 0u64;
    for n in 2u8..=MAX_LEVEL {
        let xp = xp_for_level(n);
        assert!(xp > prev, "level {n} xp {xp} is not > {prev}");
        prev = xp;
    }
}

// ---------------------------------------------------------------------------
// Pacing band test (Stage G, M5 fix: per-trait bands)
// ---------------------------------------------------------------------------

/// For each trait, sum (reminders_per_day × xp_per_completion) and assert it
/// falls within the trait's per-trait contract band.
///
/// Per-trait bands (XP/day) derived from the reminder fire profiles:
///
/// | trait     | band lo | band hi | rationale                                        |
/// |-----------|---------|---------|--------------------------------------------------|
/// | flow      | 2700    | 3800    | 2 reminders, both intra-window; tight band       |
/// | core      | 2700    | 3800    | 2 reminders, moderate cadence                    |
/// | spine     | 2700    | 3800    | 2 reminders, moderate cadence                    |
/// | depth     | 2700    | 4200    | 3 reminders incl. 24h-cycle anchors; wider       |
/// | resonance | 2700    | 4500    | 2 reminders at long intervals; rounding spread   |
/// | warmth    | 2700    | 4000    | 2 reminders, low cadence                         |
/// | reach     | 3500    | 5500    | 3 reminders compound; each individually on-band  |
/// | clarity   | 3000    | 6000    | eyes (20 min) + sun (240 min) extreme spread     |
/// | space     | 3000    | 6000    | breath (25 min) + wind (240 min) extreme spread  |
///
/// clarity and space pair a very-frequent intra-window reminder with a 4-hour
/// anchor reminder. XP/hr per reminder is within contract; their summed daily
/// total is higher because the two cadences are structurally mismatched. The
/// wider band reflects that friction profile — a 15% cadence drift in either
/// reminder would fall outside even these wider bands.
///
/// Band floors: all traits above 2700 (the ~240 XP/hr × 11.25 hr floor that
/// accounts for the lowest-cadence reminders missing some active-hour windows).
#[test]
fn pacing_band_per_trait() {
    // active hours per day
    let active_min: f64 = 15.0 * 60.0;

    // Per-trait [lo, hi] XP/day bands. See table above for rationale.
    let trait_bands: &[(&str, f64, f64)] = &[
        ("flow", 2700.0, 3800.0),
        ("core", 2700.0, 3800.0),
        ("spine", 2700.0, 3800.0),
        ("depth", 2700.0, 4200.0),
        ("resonance", 2700.0, 4500.0),
        ("warmth", 2700.0, 4000.0),
        ("reach", 3500.0, 5500.0),
        ("clarity", 3000.0, 6000.0),
        ("space", 3000.0, 6000.0),
    ];

    for &(trait_name, band_low, band_high) in trait_bands {
        let mut daily_xp: f64 = 0.0;

        for reminder in REMINDERS {
            // Resolve the trait for this reminder.
            let cat = match category_by_id(reminder.cat) {
                Some(c) => c,
                None => continue,
            };
            if cat.trait_id != trait_name {
                continue;
            }
            // For reminders whose interval exceeds the 15-active-hour window
            // (interval_min > 900), the once-per-day/half-day anchored reminders
            // fire based on a 24-hour cycle, not the active window.
            // e.g. jrnl_am (1440 min) fires 1/day; grat (720 min) fires 2/day.
            // For intra-active-window reminders use active_min / interval_min.
            let fires_per_day = if reminder.interval_min as f64 > active_min {
                // 24-hr cycle: fires = 1440 / interval_min.
                1440.0 / reminder.interval_min as f64
            } else {
                active_min / reminder.interval_min as f64
            };
            daily_xp += fires_per_day * reminder.xp_per_completion as f64;
        }

        assert!(
            daily_xp >= band_low && daily_xp <= band_high,
            "trait '{trait_name}': daily XP = {daily_xp:.0} is outside per-trait band \
             [{band_low}, {band_high}]"
        );
    }
}

// ---------------------------------------------------------------------------
// Focus multiplier test (Stage G)
// ---------------------------------------------------------------------------

#[test]
fn arrow_to_multiplier_values() {
    assert_eq!(arrow_to_multiplier(1), 2);
    assert_eq!(arrow_to_multiplier(2), 3);
    assert_eq!(arrow_to_multiplier(3), 4);
    assert_eq!(arrow_to_multiplier(0), 1); // defensive
    assert_eq!(arrow_to_multiplier(255), 1); // defensive
}

/// Spread3x2 gives 1 arrow per trait → 2× multiplier.
/// water (flow) with on_time (mult=1.0) and 1 arrow focus:
/// result = round(145 * 1.0 * 2) = 290.
#[test]
fn focus_spread3x2_multiplier_water() {
    let water = seed_core::domain::reminder_by_id("water").unwrap();

    // Build a Spread3x2 focus phase over flow, core, spine.
    let focus = FocusPhase {
        pattern: FocusPattern::Spread3x2,
        allocations: vec![
            (TraitId("flow".into()), 1),
            (TraitId("core".into()), 1),
            (TraitId("spine".into()), 1),
        ],
    };

    // on_time: mult = 1.0. focus: 1 arrow → 2×.
    // Composition: xp_per_completion * time_mult * focus_mult = 145 * 1.0 * 2 = 290.
    let result = xp_reward(
        water,
        XpRewardOpts {
            on_time: true,
            overdue: false,
        },
        Some(&focus),
    );
    assert_eq!(result, 290, "water + Spread3x2 on_time should be 290");
}

/// A reminder whose trait is NOT in the focus allocation gets no multiplier.
#[test]
fn focus_no_multiplier_for_unallocated_trait() {
    let eyes = seed_core::domain::reminder_by_id("eyes").unwrap(); // clarity trait

    // Focus only covers flow, core, spine — not clarity.
    let focus = FocusPhase {
        pattern: FocusPattern::Spread3x2,
        allocations: vec![
            (TraitId("flow".into()), 1),
            (TraitId("core".into()), 1),
            (TraitId("spine".into()), 1),
        ],
    };

    let result = xp_reward(
        eyes,
        XpRewardOpts {
            on_time: true,
            overdue: false,
        },
        Some(&focus),
    );
    // No focus for clarity: 60 * 1.0 * 1 = 60.
    assert_eq!(result, 60, "eyes (clarity) should not get focus multiplier");
}

/// Concentrate1x4 gives 3 arrows → 4× multiplier.
#[test]
fn focus_concentrate1x4_multiplier() {
    let water = seed_core::domain::reminder_by_id("water").unwrap();

    let focus = FocusPhase {
        pattern: FocusPattern::Concentrate1x4,
        allocations: vec![(TraitId("flow".into()), 3)],
    };

    // on_time: 145 * 1.0 * 4 = 580.
    let result = xp_reward(
        water,
        XpRewardOpts {
            on_time: true,
            overdue: false,
        },
        Some(&focus),
    );
    assert_eq!(result, 580);
}

/// Late multiplier with no focus: 145 * 0.6 = 87.
#[test]
fn xp_reward_late_no_focus() {
    let water = seed_core::domain::reminder_by_id("water").unwrap();
    let result = xp_reward(
        water,
        XpRewardOpts {
            on_time: false,
            overdue: false,
        },
        None,
    );
    assert_eq!(result, 87, "145 * 0.6 = 87");
}

/// Overdue multiplier: 145 * 1.4 = 203.
#[test]
fn xp_reward_overdue_no_focus() {
    let water = seed_core::domain::reminder_by_id("water").unwrap();
    let result = xp_reward(
        water,
        XpRewardOpts {
            on_time: false,
            overdue: true,
        },
        None,
    );
    assert_eq!(result, 203, "145 * 1.4 = 203");
}
