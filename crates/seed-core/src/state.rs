/// App state types and constructors.
/// Ported from `wellness/state.jsx::initialState` and `midJourneyState`.
use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::{
    domain::{FocusPhase, IntegrationEnhancement, REMINDERS, ReminderId, TraitId},
    levels::xp_for_level,
};

// ---------------------------------------------------------------------------
// Per-trait skip statistics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraitSkipStats {
    /// Total non-snoozed skips for this trait since the companion was awakened.
    pub lifetime: u32,
    /// Epoch-ms timestamps of recent non-snoozed skips. Pruned at apply time
    /// to keep only entries within the last 7 days.
    pub recent: Vec<i64>,
}

impl TraitSkipStats {
    /// Count of skips within the last 7 days relative to `now_ms`.
    pub fn count_7d(&self, now_ms: i64) -> u32 {
        let cutoff = now_ms - 7 * 86_400 * 1000;
        self.recent.iter().filter(|&&ts| ts >= cutoff).count() as u32
    }
}

// ---------------------------------------------------------------------------
// Log entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Display timestamp string (e.g. `"09:42"`).
    pub t: String,
    pub msg: String,
    pub tag: String,
}

// ---------------------------------------------------------------------------
// Per-reminder runtime
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderRuntime {
    pub enabled: bool,
    pub pinned: bool,
    /// Interval override in minutes. Falls back to static default if not set.
    pub interval_min: u32,
    /// Unix timestamp (ms) of last completion.
    pub last_done_ms: i64,
    /// Unix timestamp (ms); 0 = not snoozed.
    pub snoozed_until_ms: i64,
    pub streak: u32,
    pub total_done: u32,
    pub total_missed: u32,
    /// Unix timestamp (ms) of the last notification fired for this reminder; 0 = never.
    /// Used by the scheduler to debounce notifications within a due window.
    #[serde(default)]
    pub last_notified_ms: i64,
}

// ---------------------------------------------------------------------------
// Full state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    /// Unix timestamp (ms) when the companion was first awakened.
    pub awakened_at: i64,
    /// XP per trait.
    pub traits: BTreeMap<TraitId, u64>,
    pub reminders: BTreeMap<ReminderId, ReminderRuntime>,
    /// Deterministic seed for glyph generation.
    pub glyph_seed: u64,
    pub active_hours: (u8, u8),
    pub snooze_min: u32,
    pub notif_style: String,
    pub palette: String,
    /// Recent log lines (capped at 40 on persist, unbounded in memory).
    pub log: VecDeque<LogEntry>,
    pub last_tick_ms: i64,
    pub completed_total: u32,
    pub missed_total: u32,
    /// XP multiplier applied on every reminder completion. Default 1 (real-time).
    /// Persisted so it survives restart.
    #[serde(default = "default_xp_multiplier")]
    pub xp_multiplier: u32,

    // -----------------------------------------------------------------------
    // Integrate prestige (per-trait, data only — UI deferred)
    // -----------------------------------------------------------------------
    /// Number of times each trait has been integrated (reset from lvl 99 to 1).
    /// Absent keys default to 0. Uses `#[serde(default)]` for backward compat —
    /// old snapshots that lack this field deserialize cleanly.
    #[serde(default)]
    pub trait_integrations: BTreeMap<TraitId, u8>,

    /// Visual enhancements accumulated per trait across integrations (append-only).
    /// Each integration appends the chosen `IntegrationEnhancement` to the vec.
    #[serde(default)]
    pub trait_enhancements: BTreeMap<TraitId, Vec<IntegrationEnhancement>>,

    // -----------------------------------------------------------------------
    // Focus prestige (companion-level, data + multiplier hook — UI deferred)
    // -----------------------------------------------------------------------
    /// Monotonic sum of every level-up event ever observed for this companion.
    /// Does NOT decrement on integrate — the achievement of reaching those levels
    /// is permanent. Token balance = `cumulative_levels_gained / 99 - tokens_spent`.
    #[serde(default)]
    pub cumulative_levels_gained: u32,

    /// How many focus tokens have been spent (each activation increments this).
    #[serde(default)]
    pub tokens_spent: u32,

    /// Currently active focus phase, if any. `None` until the user spends their
    /// first token. Replaced (not stacked) on each `FocusPhaseActivated` event.
    #[serde(default)]
    pub active_focus: Option<FocusPhase>,

    // -----------------------------------------------------------------------
    // Per-trait skip tracking
    // -----------------------------------------------------------------------
    /// Non-snoozed skip statistics keyed by trait. Absent keys default to zero.
    /// `#[serde(default)]` ensures old snapshots without this field deserialize cleanly.
    #[serde(default)]
    pub traits_skipped: BTreeMap<TraitId, TraitSkipStats>,
}

fn default_xp_multiplier() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

fn default_runtime(interval_min: u32, now_ms: i64) -> ReminderRuntime {
    let partial_elapsed = (interval_min as i64 * 60 * 1000 * 40) / 100;
    ReminderRuntime {
        enabled: true,
        pinned: false,
        interval_min,
        last_done_ms: now_ms - partial_elapsed,
        snoozed_until_ms: 0,
        streak: 0,
        total_done: 0,
        total_missed: 0,
        last_notified_ms: 0,
    }
}

/// Initial state: all traits at 0 XP, all reminders enabled with partial progress.
/// `now_ms` is injected so the function stays pure (no clock I/O).
pub fn initial_state(now_ms: i64) -> State {
    let mut reminders: BTreeMap<ReminderId, ReminderRuntime> = BTreeMap::new();
    for r in REMINDERS {
        reminders.insert(r.reminder_id(), default_runtime(r.interval_min, now_ms));
    }

    let mut traits: BTreeMap<TraitId, u64> = BTreeMap::new();
    for name in &[
        "flow",
        "core",
        "spine",
        "reach",
        "clarity",
        "space",
        "depth",
        "resonance",
        "warmth",
    ] {
        traits.insert(TraitId(name.to_string()), 0);
    }

    State {
        awakened_at: now_ms,
        traits,
        reminders,
        glyph_seed: 42,
        active_hours: (7, 22),
        snooze_min: 5,
        notif_style: "flash".to_string(),
        palette: "sage".to_string(),
        log: VecDeque::from([LogEntry {
            t: "00:00".to_string(),
            msg: "companion awakened.".to_string(),
            tag: "accent".to_string(),
        }]),
        last_tick_ms: now_ms,
        completed_total: 0,
        missed_total: 0,
        xp_multiplier: 1,
        trait_integrations: BTreeMap::new(),
        trait_enhancements: BTreeMap::new(),
        cumulative_levels_gained: 0,
        tokens_spent: 0,
        active_focus: None,
        traits_skipped: BTreeMap::new(),
    }
}

/// Mid-journey state: traits seeded at levels ~9-24, a few reminders pinned
/// and staggered, matching `state.jsx::midJourneyState`.
/// `now_ms` injected for purity.
///
/// # Panics
/// Panics with a clear message if a trait name is not present in `initial_state`.
/// This is a development fixture — the catalog is static and the names are
/// hard-coded. A panic here means the catalog was edited without updating this
/// function, which is a programming error, not a runtime condition.
pub fn mid_journey_state(now_ms: i64) -> State {
    let mut s = initial_state(now_ms);

    let set = |traits: &mut BTreeMap<TraitId, u64>, name: &str, val: u64| {
        let entry = traits
            .get_mut(&TraitId(name.to_string()))
            .unwrap_or_else(|| {
                panic!(
                    "mid_journey_state: trait '{name}' not found in initial_state — \
                 update mid_journey_state to match the current trait catalog"
                )
            });
        *entry = val;
    };

    set(&mut s.traits, "flow", xp_for_level(24) + 200);
    set(&mut s.traits, "core", xp_for_level(18) + 120);
    set(&mut s.traits, "spine", xp_for_level(15) + 40);
    set(&mut s.traits, "reach", xp_for_level(22) + 180);
    set(&mut s.traits, "clarity", xp_for_level(17) + 90);
    set(&mut s.traits, "space", xp_for_level(20) + 50);
    set(&mut s.traits, "depth", xp_for_level(11) + 60);
    set(&mut s.traits, "resonance", xp_for_level(9) + 20);
    set(&mut s.traits, "warmth", xp_for_level(13) + 30);

    s.completed_total = 47;
    s.missed_total = 6;

    // Deterministic streak + history (not random — pure function)
    for (i, rt) in s.reminders.values_mut().enumerate() {
        rt.streak = 3 + (i as u32 % 6);
        rt.total_done = 3 + (i as u32 % 10);
    }

    // Pre-pin six reminders
    for id in &["water", "eyes", "stand", "breath", "walk", "jrnl_am"] {
        if let Some(rt) = s.reminders.get_mut(&ReminderId(id.to_string())) {
            rt.pinned = true;
        }
    }

    // Stagger timers so some are due
    if let Some(rt) = s.reminders.get_mut(&ReminderId("water".to_string())) {
        rt.last_done_ms = now_ms - 42 * 60 * 1000;
    }
    if let Some(rt) = s.reminders.get_mut(&ReminderId("eyes".to_string())) {
        rt.last_done_ms = now_ms - 19 * 60 * 1000;
    }
    if let Some(rt) = s.reminders.get_mut(&ReminderId("stretch".to_string())) {
        rt.last_done_ms = now_ms - 55 * 60 * 1000;
    }

    s.log = VecDeque::from([
        LogEntry {
            t: "00:00".to_string(),
            msg: "water · logged. +flow xp".to_string(),
            tag: "accent".to_string(),
        },
        LogEntry {
            t: "00:01".to_string(),
            msg: "eyes rested. +clarity xp".to_string(),
            tag: "dim".to_string(),
        },
        LogEntry {
            t: "00:02".to_string(),
            msg: "companion unfurls.".to_string(),
            tag: "accent-2".to_string(),
        },
    ]);

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tier_for;
    use crate::levels::level_for_xp;

    const NOW: i64 = 1_745_000_000_000;

    #[test]
    fn initial_state_has_nine_traits() {
        let s = initial_state(NOW);
        assert_eq!(s.traits.len(), 9);
    }

    #[test]
    fn initial_state_has_twenty_reminders() {
        let s = initial_state(NOW);
        assert_eq!(s.reminders.len(), 20);
    }

    #[test]
    fn initial_traits_are_zero_xp() {
        let s = initial_state(NOW);
        for (id, xp) in &s.traits {
            assert_eq!(*xp, 0, "trait {id:?} should start at 0");
        }
    }

    #[test]
    fn mid_journey_total_level_is_frond_or_bloom() {
        let s = mid_journey_state(NOW);
        let total: u32 = s.traits.values().map(|&xp| level_for_xp(xp) as u32).sum();
        let tier = tier_for(total);
        assert!(
            tier == crate::domain::Tier::Frond || tier == crate::domain::Tier::Bloom,
            "expected Frond or Bloom, got {tier:?} (total_level={total})"
        );
    }

    #[test]
    fn mid_journey_six_reminders_pinned() {
        let s = mid_journey_state(NOW);
        let pinned_count = s.reminders.values().filter(|rt| rt.pinned).count();
        assert_eq!(pinned_count, 6);
    }

    #[test]
    fn state_round_trips_json() {
        let s = initial_state(NOW);
        let json = serde_json::to_string(&s).unwrap();
        let s2: State = serde_json::from_str(&json).unwrap();
        assert_eq!(s.completed_total, s2.completed_total);
        assert_eq!(s.traits.len(), s2.traits.len());
    }
}
