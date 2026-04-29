/// OSRS XP curve, ported from `wellness/levels.jsx::buildXpTable`.
///
/// Base table: level 99 = 13,034,431 XP. Level 92 ≈ half of 99.
/// Formula: xp(L) = floor( (1/4) * Σ_{x=1..L-1} floor(x + 300 · 2^(x/7)) )
///
/// `SCALE_DIVISOR` compresses the table so level 99 = ~1,303,443 XP.
/// This preserves the curve shape (lvl 92 ≈ half of lvl 99) while making
/// per-fire rewards legible 2–4 digit numbers that align with the 1-year
/// time-to-99 contract (see `docs/specs/xp-pacing.md`).
use std::sync::OnceLock;

use crate::domain::{FocusPhase, Reminder, category_by_id};

pub const MAX_LEVEL: u8 = 99;

/// Divisor applied to the raw OSRS XP table. Scales level 99 from 13,034,431
/// down to ~1,303,443 so per-reminder rewards are legible integers.
const SCALE_DIVISOR: u64 = 10;

static XP_TABLE: OnceLock<Vec<u64>> = OnceLock::new();

fn xp_table() -> &'static Vec<u64> {
    XP_TABLE.get_or_init(|| {
        // index 0 unused; index 1 = 0; index 2..=99 computed
        let mut table = vec![0u64; (MAX_LEVEL as usize) + 1];
        let mut points: f64 = 0.0;
        for lvl in 1u64..(MAX_LEVEL as u64) {
            points += f64::floor(lvl as f64 + 300.0 * f64::powf(2.0, lvl as f64 / 7.0));
            // Divide by SCALE_DIVISOR and round (using floor on the already-floored value
            // means we take floor of the division, matching integer truncation).
            table[(lvl + 1) as usize] =
                f64::floor(f64::floor(points / 4.0) / SCALE_DIVISOR as f64) as u64;
        }
        table
    })
}

/// XP required to reach `level`. Level 1 requires 0 XP.
pub fn xp_for_level(level: u8) -> u64 {
    let table = xp_table();
    if level <= 1 {
        return 0;
    }
    if level >= MAX_LEVEL {
        return table[MAX_LEVEL as usize];
    }
    table[level as usize]
}

/// Current level for a given XP total. Returns 1 for xp=0, capped at MAX_LEVEL.
pub fn level_for_xp(xp: u64) -> u8 {
    if xp == 0 {
        return 1;
    }
    let table = xp_table();
    let mut lvl: u8 = 1;
    for i in 2u8..=MAX_LEVEL {
        if xp >= table[i as usize] {
            lvl = i;
        } else {
            break;
        }
    }
    lvl
}

/// Progress within the current level, 0.0..=1.0.
/// Returns 1.0 at max level.
pub fn level_progress(xp: u64) -> f32 {
    let lvl = level_for_xp(xp);
    if lvl >= MAX_LEVEL {
        return 1.0;
    }
    let cur = xp_for_level(lvl);
    let next = xp_for_level(lvl + 1);
    if next == cur {
        return 1.0;
    }
    ((xp - cur) as f64 / (next - cur) as f64) as f32
}

/// XP needed to reach the next level. Returns 0 at max level.
pub fn xp_to_next(xp: u64) -> u64 {
    let lvl = level_for_xp(xp);
    if lvl >= MAX_LEVEL {
        return 0;
    }
    xp_for_level(lvl + 1).saturating_sub(xp)
}

/// Normalized level in 0.0..=1.0 (level / MAX_LEVEL). Used by glyph renderer.
pub fn level_norm(xp: u64) -> f32 {
    (level_for_xp(xp) as f32 / MAX_LEVEL as f32).min(1.0)
}

/// Options for `xp_reward`.
#[derive(Debug, Default, Clone, Copy)]
pub struct XpRewardOpts {
    pub overdue: bool,
    pub on_time: bool,
}

/// Map arrow count (1/2/3) to XP multiplier (2×/3×/4×).
///
/// Arrows represent the focus intensity for a trait in an active `FocusPhase`:
/// - 1 arrow = `Spread3x2` — 2× multiplier (broadest spread)
/// - 2 arrows = `Spread2x3` — 3× multiplier
/// - 3 arrows = `Concentrate1x4` — 4× multiplier (peak rate)
///
/// 0 arrows (or any other value) returns 1 as a defensive default — no boost.
pub fn arrow_to_multiplier(arrows: u8) -> u32 {
    match arrows {
        1 => 2,
        2 => 3,
        3 => 4,
        _ => 1,
    }
}

/// XP reward for completing a reminder.
///
/// Composition order: `xp_per_completion × time_mult × focus_mult`
/// - `time_mult`: 0.6 (late) / 1.0 (on_time) / 1.4 (overdue) — tightened from the
///   original 0.55/1.35/2.0 so punctuality is the canonical reward.
/// - `focus_mult`: `arrow_to_multiplier(arrows)` if `focus` has an allocation for
///   the reminder's trait; 1 (no-op) otherwise.
///
/// Deterministic — no jitter. Callers may add jitter at the call site if desired.
pub fn xp_reward(reminder: &Reminder, opts: XpRewardOpts, focus: Option<&FocusPhase>) -> u32 {
    // Step 1: base XP baked into the static reminder catalog.
    let base = reminder.xp_per_completion as f64;

    // Step 2: timing multiplier. Tightened from 0.55/1.35/2.0 to 0.6/1.0/1.4.
    // "on_time" is the canonical rate; overdue retains a small comeback bonus;
    // late has a soft penalty. Neither extreme games the system.
    let time_mult = if opts.overdue {
        1.4
    } else if opts.on_time {
        1.0
    } else {
        0.6
    };

    // Step 3: focus multiplier — only applied when an active phase allocates
    // arrows to this reminder's trait.
    let focus_mult = focus
        .and_then(|f| {
            // Resolve the trait for this reminder's category.
            let trait_id = category_by_id(reminder.cat)?.trait_id;
            // Find the allocation for that trait.
            let arrows = f.allocations.iter().find(|(t, _)| t.0 == trait_id)?.1;
            Some(arrow_to_multiplier(arrows))
        })
        .unwrap_or(1) as f64;

    f64::round(base * time_mult * focus_mult) as u32
}

/// XP drain per tick for a missed reminder (matches `xpDrain` in `levels.jsx`).
/// Stored as a scaled integer: drain = 35 means 0.35 XP. Callers accumulate and
/// apply as `total_drain / 100`.
pub fn xp_drain() -> u32 {
    // JSX returns 0.35; we return 35 (× 100) to stay integer.
    35
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_1_is_zero_xp() {
        assert_eq!(xp_for_level(1), 0);
    }

    #[test]
    fn level_99_canonical() {
        // OSRS base ÷ SCALE_DIVISOR (10). Floor division introduces rounding vs
        // the raw value 13,034,431 / 10 = 1,303,443.1 → floor = 1,303,443.
        assert_eq!(xp_for_level(99), 1_303_443);
    }

    #[test]
    fn level_92_is_roughly_half_of_99() {
        let xp92 = xp_for_level(92) as f64;
        let xp99 = xp_for_level(99) as f64;
        let ratio = xp92 / xp99;
        // Actual ratio ≈ 0.5007 after SCALE_DIVISOR=10 floor division.
        // 0.01 epsilon is sufficient (0.5007 - 0.5 = 0.0007 < 0.01).
        assert!(
            (ratio - 0.5).abs() < 0.01,
            "xp_for_level(92)/xp_for_level(99) = {ratio:.4}, expected ~0.5"
        );
    }

    #[test]
    fn round_trip_level_for_xp() {
        for n in 1u8..=99 {
            let xp = xp_for_level(n);
            assert_eq!(level_for_xp(xp), n, "round-trip failed at level {n}");
        }
    }

    #[test]
    fn level_progress_at_max() {
        assert_eq!(level_progress(xp_for_level(99)), 1.0);
    }

    #[test]
    fn xp_to_next_at_max_is_zero() {
        assert_eq!(xp_to_next(xp_for_level(99)), 0);
    }

    #[test]
    fn level_norm_at_max_is_one() {
        assert!((level_norm(xp_for_level(99)) - 1.0).abs() < f32::EPSILON);
    }
}
