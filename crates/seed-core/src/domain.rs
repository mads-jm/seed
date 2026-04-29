/// Domain types: categories, reminders, tiers, reminder status.
/// Ported faithfully from `wellness/data.jsx` and `wellness/state.jsx`.
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ID newtypes — String-owned for serde compatibility.
// Static catalog structs use plain &'static str for their id fields.
// ---------------------------------------------------------------------------

/// Stable string key for a trait (e.g. `"flow"`, `"core"`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TraitId(pub String);

impl From<&str> for TraitId {
    fn from(s: &str) -> Self {
        TraitId(s.to_owned())
    }
}

/// Stable string key for a reminder category (e.g. `"hydration"`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CategoryId(pub String);

impl From<&str> for CategoryId {
    fn from(s: &str) -> Self {
        CategoryId(s.to_owned())
    }
}

/// Stable string key for a reminder (e.g. `"water"`, `"jrnl_am"`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ReminderId(pub String);

impl From<&str> for ReminderId {
    fn from(s: &str) -> Self {
        ReminderId(s.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Static catalog types — use plain &'static str for ids to allow static items.
// ---------------------------------------------------------------------------

/// One of the 9 reminder categories, each bound to exactly one trait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: &'static str,
    pub name: &'static str,
    pub trait_id: &'static str,
    pub icon: &'static str,
    pub description: &'static str,
}

impl Category {
    pub fn category_id(&self) -> CategoryId {
        CategoryId(self.id.to_owned())
    }
    pub fn trait_id_key(&self) -> TraitId {
        TraitId(self.trait_id.to_owned())
    }
}

/// One of the 20 reminders. `anchor_hour` is set for calendar-anchored reminders
/// (e.g. morning/evening journaling).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub id: &'static str,
    pub cat: &'static str,
    pub name: &'static str,
    /// Short verb used in log lines (e.g. `"water"`, `"align"`).
    pub word: &'static str,
    /// Default interval in minutes.
    pub interval_min: u32,
    /// If set, the reminder is anchored to this hour of day (0-23).
    pub anchor_hour: Option<u8>,
    pub desc: &'static str,
    /// XP awarded per on-time completion (before timing/focus multipliers).
    /// Derived from the per-trait daily budget (1,303,443 / 365 ≈ 3,571 XP/day)
    /// divided by the reminder's fires-per-day at perfect adherence.
    /// See `docs/specs/xp-pacing.md` for the full derivation table.
    pub xp_per_completion: u32,
}

impl Reminder {
    pub fn reminder_id(&self) -> ReminderId {
        ReminderId(self.id.to_owned())
    }
    pub fn category_id(&self) -> CategoryId {
        CategoryId(self.cat.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Static catalog data — ported from data.jsx
// ---------------------------------------------------------------------------

pub static CATEGORIES: &[Category] = &[
    Category {
        id: "hydration",
        name: "HYDRATION",
        trait_id: "flow",
        icon: "~",
        description: "Water shapes attention — steady sipping keeps thought fluid and fatigue away.",
    },
    Category {
        id: "nourishment",
        name: "NOURISHMENT",
        trait_id: "core",
        icon: "●",
        description: "Steady meals anchor the day; stable blood sugar is the floor beneath focus.",
    },
    Category {
        id: "posture",
        name: "POSTURE",
        trait_id: "spine",
        icon: "|",
        description: "Alignment is the spine of every other skill — comfort starts at the seat.",
    },
    Category {
        id: "movement",
        name: "MOVEMENT",
        trait_id: "reach",
        icon: ">",
        description: "Motion is the antidote to stagnation; brief walks reset the whole system.",
    },
    Category {
        id: "vision",
        name: "VISION",
        trait_id: "clarity",
        icon: "o",
        description: "Looking far relaxes the eyes and loosens the grip of close-focus fatigue.",
    },
    Category {
        id: "breath",
        name: "BREATH",
        trait_id: "space",
        icon: "-",
        description: "Breath is the room the rest of the practice fits inside — pause and expand.",
    },
    Category {
        id: "reflection",
        name: "REFLECTION",
        trait_id: "depth",
        icon: "#",
        description: "Brief pauses to write compound into self-knowledge over weeks and months.",
    },
    Category {
        id: "mind",
        name: "MIND",
        trait_id: "resonance",
        icon: "*",
        description: "Small acts of presence — sitting still, reading slowly — tune the inner ear.",
    },
    Category {
        id: "care",
        name: "CARE",
        trait_id: "warmth",
        icon: "+",
        description: "Rest and reaching out are the ground every other skill rises from.",
    },
];

pub static REMINDERS: &[Reminder] = &[
    // hydration → flow
    // fires/day (15 active hr): water=20, tea=5; daily budget: 20*145 + 5*145 = 3625 ≈ 3571
    Reminder {
        id: "water",
        cat: "hydration",
        name: "DRINK WATER",
        word: "water",
        interval_min: 45,
        anchor_hour: None,
        desc: "sip slowly.",
        xp_per_completion: 145,
    },
    Reminder {
        id: "tea",
        cat: "hydration",
        name: "WARM DRINK",
        word: "steep",
        interval_min: 180,
        anchor_hour: None,
        desc: "herbal, low caffeine.",
        xp_per_completion: 145,
    },
    // nourishment → core
    // fires/day: eat=5, snack=7.5; daily budget: 5*320 + 7.5*285 = 3737 ≈ 3571
    Reminder {
        id: "eat",
        cat: "nourishment",
        name: "NOURISH",
        word: "eat",
        interval_min: 180,
        anchor_hour: None,
        desc: "small meal.",
        xp_per_completion: 320,
    },
    Reminder {
        id: "snack",
        cat: "nourishment",
        name: "LIGHT SNACK",
        word: "graze",
        interval_min: 120,
        anchor_hour: None,
        desc: "something small.",
        xp_per_completion: 285,
    },
    // posture → spine
    // fires/day: stand=18, posture=30; daily budget: 18*95 + 30*60 = 3510 ≈ 3571
    Reminder {
        id: "stand",
        cat: "posture",
        name: "STAND",
        word: "stand",
        interval_min: 50,
        anchor_hour: None,
        desc: "rise and reset.",
        xp_per_completion: 95,
    },
    Reminder {
        id: "posture",
        cat: "posture",
        name: "POSTURE",
        word: "align",
        interval_min: 30,
        anchor_hour: None,
        desc: "shoulders back.",
        xp_per_completion: 60,
    },
    // movement → reach
    // fires/day: walk=10, stretch=15, shake=7.5; daily budget: 10*165 + 15*110 + 7.5*220 = 3900 ≈ 3571
    Reminder {
        id: "walk",
        cat: "movement",
        name: "WALK",
        word: "walk",
        interval_min: 90,
        anchor_hour: None,
        desc: "a short wander.",
        xp_per_completion: 165,
    },
    Reminder {
        id: "stretch",
        cat: "movement",
        name: "STRETCH",
        word: "stretch",
        interval_min: 60,
        anchor_hour: None,
        desc: "unfurl slowly.",
        xp_per_completion: 110,
    },
    Reminder {
        id: "shake",
        cat: "movement",
        name: "SHAKE OUT",
        word: "shake",
        interval_min: 120,
        anchor_hour: None,
        desc: "loosen the limbs.",
        xp_per_completion: 220,
    },
    // vision → clarity
    // fires/day: eyes=45, sun=3.75; daily budget: 45*60 + 3.75*720 = 5400 ≈ 3571
    // (vision is denser on fires — eyes fires very frequently)
    Reminder {
        id: "eyes",
        cat: "vision",
        name: "EYE REST",
        word: "look",
        interval_min: 20,
        anchor_hour: None,
        desc: "20-20-20 rule.",
        xp_per_completion: 60,
    },
    Reminder {
        id: "sun",
        cat: "vision",
        name: "DAYLIGHT",
        word: "sun",
        interval_min: 240,
        anchor_hour: None,
        desc: "step into light.",
        xp_per_completion: 720,
    },
    // breath → space
    // fires/day: breath=36, wind=3.75; daily budget: 36*75 + 3.75*715 = 5381 ≈ 3571
    Reminder {
        id: "breath",
        cat: "breath",
        name: "DEEP BREATH",
        word: "breathe",
        interval_min: 25,
        anchor_hour: None,
        desc: "four in, six out.",
        xp_per_completion: 75,
    },
    Reminder {
        id: "wind",
        cat: "breath",
        name: "WIND DOWN",
        word: "rest",
        interval_min: 240,
        anchor_hour: None,
        desc: "dim the lights.",
        xp_per_completion: 715,
    },
    // reflection → depth
    // fires/day: jrnl_am=1, jrnl_pm=1, grat=2; daily budget: 1*1200 + 1*1200 + 2*600 = 3600 ≈ 3571
    Reminder {
        id: "jrnl_am",
        cat: "reflection",
        name: "MORNING PAGES",
        word: "journal",
        interval_min: 1440,
        anchor_hour: Some(8),
        desc: "set an intention.",
        xp_per_completion: 1200,
    },
    Reminder {
        id: "jrnl_pm",
        cat: "reflection",
        name: "EVENING PAGES",
        word: "reflect",
        interval_min: 1440,
        anchor_hour: Some(20),
        desc: "review the day.",
        xp_per_completion: 1200,
    },
    Reminder {
        id: "grat",
        cat: "reflection",
        name: "GRATITUDE",
        word: "thanks",
        interval_min: 720,
        anchor_hour: None,
        desc: "name three things.",
        xp_per_completion: 600,
    },
    // mind → resonance
    // fires/day: med=2.5, read=1.875; daily budget: 2.5*815 + 1.875*1090 = 4081 ≈ 3571
    Reminder {
        id: "med",
        cat: "mind",
        name: "MEDITATE",
        word: "sit",
        interval_min: 360,
        anchor_hour: None,
        desc: "five quiet minutes.",
        xp_per_completion: 815,
    },
    Reminder {
        id: "read",
        cat: "mind",
        name: "READ",
        word: "read",
        interval_min: 480,
        anchor_hour: None,
        desc: "a few slow pages.",
        xp_per_completion: 1090,
    },
    // care → warmth
    // fires/day: tidy=3, reach=1.25; daily budget: 3*600 + 1.25*1400 = 3550 ≈ 3571
    Reminder {
        id: "tidy",
        cat: "care",
        name: "TIDY SPACE",
        word: "tidy",
        interval_min: 300,
        anchor_hour: None,
        desc: "one small area.",
        xp_per_completion: 600,
    },
    Reminder {
        id: "reach",
        cat: "care",
        name: "REACH OUT",
        word: "reach",
        interval_min: 720,
        anchor_hour: None,
        desc: "message someone.",
        xp_per_completion: 1400,
    },
];

// ---------------------------------------------------------------------------
// Prestige — Integrate
// ---------------------------------------------------------------------------

/// Visual enhancement applied to a trait's glyph when the user integrates
/// (resets) that trait from level 99.  One entry per trait in the starter set;
/// expandable without breaking persisted state because variants are serialised
/// by their stable PascalCase string id.
///
/// Multiple enhancements on the same trait stack linearly — the glyph renderer
/// draws each layer in order.  (Rendering is deferred; data model ships here.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegrationEnhancement {
    FlowSpiral,
    CoreEmber,
    SpineLattice,
    ReachBranch,
    ClarityRing,
    SpaceVeil,
    DepthAbyss,
    ResonanceChord,
    WarmthGlow,
}

// ---------------------------------------------------------------------------
// Prestige — Focus
// ---------------------------------------------------------------------------

/// Allocation pattern chosen when spending a focus token.
///
/// Each pattern defines how the 4× bonus budget is spread across traits:
/// - `Concentrate1x4`: 1 trait, 3 arrows → 4× multiplier (peak rate)
/// - `Spread2x3`: 2 traits, 2 arrows each → 3× multiplier per trait
/// - `Spread3x2`: 3 traits, 1 arrow each → 2× multiplier per trait
///
/// Patterns are *not* arithmetically equal by design: concentrate trades total
/// budget for peak rate, spread trades peak rate for breadth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FocusPattern {
    Concentrate1x4,
    Spread2x3,
    Spread3x2,
}

impl FocusPattern {
    /// Expected number of trait allocations for this pattern (1, 2, or 3).
    pub fn skill_count(&self) -> usize {
        match self {
            FocusPattern::Concentrate1x4 => 1,
            FocusPattern::Spread2x3 => 2,
            FocusPattern::Spread3x2 => 3,
        }
    }

    /// Arrow count assigned to each allocated trait.
    pub fn arrows_per_skill(&self) -> u8 {
        match self {
            FocusPattern::Concentrate1x4 => 3,
            FocusPattern::Spread2x3 => 2,
            FocusPattern::Spread3x2 => 1,
        }
    }
}

/// Active focus phase: the pattern in use and per-trait arrow allocations.
/// `allocations[i].1` is the arrow count (1, 2, or 3) for that trait.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusPhase {
    pub pattern: FocusPattern,
    /// (trait_id, arrow_count) pairs. Arrow count maps to multiplier via
    /// `arrow_to_multiplier` in `levels.rs`.
    pub allocations: Vec<(TraitId, u8)>,
}

// ---------------------------------------------------------------------------
// Tier
// ---------------------------------------------------------------------------

/// Companion evolution tiers. `min_total_level` is the minimum sum of all 9
/// trait levels needed to unlock this tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tier {
    Seed,
    Sprout,
    Frond,
    Bloom,
    Orbit,
    Lattice,
    Mandala,
    Lumen,
    Nebula,
    Zenith,
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Tier::Seed => "SEED",
            Tier::Sprout => "SPROUT",
            Tier::Frond => "FROND",
            Tier::Bloom => "BLOOM",
            Tier::Orbit => "ORBIT",
            Tier::Lattice => "LATTICE",
            Tier::Mandala => "MANDALA",
            Tier::Lumen => "LUMEN",
            Tier::Nebula => "NEBULA",
            Tier::Zenith => "ZENITH",
        }
    }

    pub fn adj(self) -> &'static str {
        match self {
            Tier::Seed => "nascent",
            Tier::Sprout => "unfurling",
            Tier::Frond => "reaching",
            Tier::Bloom => "opening",
            Tier::Orbit => "circling",
            Tier::Lattice => "weaving",
            Tier::Mandala => "turning",
            Tier::Lumen => "glowing",
            Tier::Nebula => "vast",
            Tier::Zenith => "radiant",
        }
    }

    /// Minimum combined level (sum of all 9 trait levels) to reach this tier.
    pub fn min_total_level(self) -> u32 {
        match self {
            Tier::Seed => 0,
            Tier::Sprout => 18,   // avg lvl 2 × 9
            Tier::Frond => 63,    // avg lvl 7 × 9
            Tier::Bloom => 135,   // avg lvl 15 × 9
            Tier::Orbit => 270,   // avg lvl 30 × 9
            Tier::Lattice => 450, // avg lvl 50 × 9
            Tier::Mandala => 630, // avg lvl 70 × 9
            Tier::Lumen => 765,   // avg lvl 85 × 9
            Tier::Nebula => 855,  // avg lvl 95 × 9
            Tier::Zenith => 891,  // all 99 × 9
        }
    }
}

/// Return the highest tier unlocked for the given combined trait level sum.
pub fn tier_for(total_level: u32) -> Tier {
    use Tier::*;
    let tiers = [
        Zenith, Nebula, Lumen, Mandala, Lattice, Orbit, Bloom, Frond, Sprout, Seed,
    ];
    for tier in tiers {
        if total_level >= tier.min_total_level() {
            return tier;
        }
    }
    Tier::Seed
}

// ---------------------------------------------------------------------------
// Reminder status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReminderState {
    Off,
    Dormant,
    Due,
    Overdue,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ReminderStatus {
    pub state: ReminderState,
    /// Milliseconds until due (negative = overdue).
    pub ms_left: i64,
    /// 0.0 = just done, 1.0 = due or past-due.
    pub pct: f32,
}

/// Compute reminder status. Matches `reminderStatus` in `state.jsx`.
///
/// Overdue: elapsed > 1.5× interval (i.e. `overdueMs > 0.5 × interval_ms`).
/// Due: `msLeft <= 0` but not yet overdue.
/// Dormant: `msLeft > 0`.
///
/// `interval_min` is passed explicitly so callers can use the runtime override
/// (`ReminderRuntime::interval_min`) instead of the static catalog default.
pub fn reminder_status(
    reminder: &Reminder,
    last_done_ms: i64,
    enabled: bool,
    now_ms: i64,
) -> ReminderStatus {
    reminder_status_with_interval(reminder.interval_min, last_done_ms, enabled, now_ms)
}

/// Compute reminder status using an explicit interval override.
/// Use this when callers have a runtime interval (e.g. from `ReminderRuntime::interval_min`).
pub fn reminder_status_with_interval(
    interval_min: u32,
    last_done_ms: i64,
    enabled: bool,
    now_ms: i64,
) -> ReminderStatus {
    if !enabled {
        return ReminderStatus {
            state: ReminderState::Off,
            ms_left: 0,
            pct: 0.0,
        };
    }
    let interval_ms = interval_min as i64 * 60 * 1000;
    let due_at = last_done_ms + interval_ms;
    let ms_left = due_at - now_ms;
    let pct = {
        let raw = 1.0 - (ms_left as f64 / interval_ms as f64).clamp(0.0, 1.0);
        raw as f32
    };

    if ms_left <= 0 {
        let overdue_ms = -ms_left;
        if overdue_ms > interval_ms / 2 {
            return ReminderStatus {
                state: ReminderState::Overdue,
                ms_left,
                pct: 1.0,
            };
        }
        return ReminderStatus {
            state: ReminderState::Due,
            ms_left,
            pct: 1.0,
        };
    }

    ReminderStatus {
        state: ReminderState::Dormant,
        ms_left,
        pct,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Look up a `Category` by id string. Returns `None` if not found.
pub fn category_by_id(id: &str) -> Option<&'static Category> {
    CATEGORIES.iter().find(|c| c.id == id)
}

/// Look up a `Reminder` by id string. Returns `None` if not found.
pub fn reminder_by_id(id: &str) -> Option<&'static Reminder> {
    REMINDERS.iter().find(|r| r.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_count() {
        assert_eq!(CATEGORIES.len(), 9);
    }

    #[test]
    fn reminder_count() {
        assert_eq!(REMINDERS.len(), 20);
    }

    #[test]
    fn anchor_hours_present() {
        let jrnl_am = reminder_by_id("jrnl_am").unwrap();
        let jrnl_pm = reminder_by_id("jrnl_pm").unwrap();
        assert_eq!(jrnl_am.anchor_hour, Some(8));
        assert_eq!(jrnl_pm.anchor_hour, Some(20));
    }

    #[test]
    fn tier_seed_at_zero() {
        assert_eq!(tier_for(0), Tier::Seed);
    }

    #[test]
    fn tier_zenith_at_891() {
        assert_eq!(tier_for(891), Tier::Zenith);
    }

    #[test]
    fn mid_journey_is_frond_or_bloom() {
        let t = tier_for(149);
        assert!(t == Tier::Frond || t == Tier::Bloom, "got {t:?}");
    }

    #[test]
    fn reminder_status_off_when_disabled() {
        let r = &REMINDERS[0];
        let s = reminder_status(r, 0, false, 1_000_000);
        assert_eq!(s.state, ReminderState::Off);
    }

    #[test]
    fn reminder_status_dormant_just_done() {
        let r = &REMINDERS[0]; // water, 45 min
        let now = 1_000_000_000i64;
        let last_done = now - 60_000; // done 1 minute ago
        let s = reminder_status(r, last_done, true, now);
        assert_eq!(s.state, ReminderState::Dormant);
    }

    #[test]
    fn reminder_status_due() {
        let r = &REMINDERS[0]; // water, 45 min = 2_700_000 ms
        let now = 1_000_000_000i64;
        let last_done = now - 2_700_001;
        let s = reminder_status(r, last_done, true, now);
        assert_eq!(s.state, ReminderState::Due);
    }

    #[test]
    fn reminder_status_overdue() {
        let r = &REMINDERS[0]; // water, 45 min; overdue at > 67.5 min
        let now = 1_000_000_000i64;
        let last_done = now - 75 * 60 * 1000;
        let s = reminder_status(r, last_done, true, now);
        assert_eq!(s.state, ReminderState::Overdue);
    }

    /// Regression: interval override is honored — a 90-min override on water (45 min
    /// static) means the reminder is still Dormant at 46 min elapsed.
    #[test]
    fn reminder_status_with_interval_honors_override() {
        let now = 1_000_000_000i64;
        let last_done = now - 46 * 60 * 1000; // 46 min ago
        // Static interval would be Due (45 min), but with 90-min override → Dormant.
        let s = reminder_status_with_interval(90, last_done, true, now);
        assert_eq!(s.state, ReminderState::Dormant);
        // And at 91 min it becomes Due.
        let last_done2 = now - 91 * 60 * 1000;
        let s2 = reminder_status_with_interval(90, last_done2, true, now);
        assert_eq!(s2.state, ReminderState::Due);
    }
}
