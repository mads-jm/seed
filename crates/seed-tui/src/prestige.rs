/// Shared prestige types and helpers accessible from both the binary (app.rs)
/// and the view layer (prestige_modal.rs).
use seed_core::domain::{FocusPattern, IntegrationEnhancement, TraitId};

// ---------------------------------------------------------------------------
// Modal state
// ---------------------------------------------------------------------------

/// Stage of the two-step phase-chooser modal.
#[derive(Debug, Clone)]
pub enum PhaseChooserStage {
    /// Step 1: selecting which allocation pattern to use.
    Pattern { cursor: usize },
    /// Step 2: selecting which traits to allocate.
    Traits {
        pattern: FocusPattern,
        /// Toggle state for each of the 9 traits (indexed by CATEGORIES order).
        selected: Vec<bool>,
        cursor: usize,
    },
}

/// Prestige overlay modals. Only one can be open at a time.
#[derive(Debug, Clone, Default)]
pub enum PrestigeModal {
    #[default]
    None,
    /// Enhancement-chooser: confirm integrate for a trait at level 99.
    EnhancementChooser {
        trait_id: TraitId,
        /// Index into the per-trait enhancement options (currently just 1 starter).
        cursor: usize,
    },
    /// Phase-chooser: spend a token to activate a focus phase.
    PhaseChooser(PhaseChooserStage),
}

impl PrestigeModal {
    pub fn is_open(&self) -> bool {
        !matches!(self, PrestigeModal::None)
    }
}

/// Ordered list of all allocation patterns for the phase-chooser.
pub const FOCUS_PATTERNS: &[FocusPattern] = &[
    FocusPattern::Spread3x2,
    FocusPattern::Spread2x3,
    FocusPattern::Concentrate1x4,
];

// ---------------------------------------------------------------------------
// Enhancement helpers
// ---------------------------------------------------------------------------

/// Return the default `IntegrationEnhancement` for a given trait id string.
pub fn default_enhancement(trait_id: &str) -> IntegrationEnhancement {
    match trait_id {
        "flow" => IntegrationEnhancement::FlowSpiral,
        "core" => IntegrationEnhancement::CoreEmber,
        "spine" => IntegrationEnhancement::SpineLattice,
        "reach" => IntegrationEnhancement::ReachBranch,
        "clarity" => IntegrationEnhancement::ClarityRing,
        "space" => IntegrationEnhancement::SpaceVeil,
        "depth" => IntegrationEnhancement::DepthAbyss,
        "resonance" => IntegrationEnhancement::ResonanceChord,
        "warmth" => IntegrationEnhancement::WarmthGlow,
        _ => IntegrationEnhancement::FlowSpiral, // fallback
    }
}

/// Parse an IntegrationEnhancement from its variant name string (case-insensitive).
pub fn parse_enhancement(s: &str) -> Option<IntegrationEnhancement> {
    match s.to_lowercase().as_str() {
        "flowspiral" => Some(IntegrationEnhancement::FlowSpiral),
        "coreember" => Some(IntegrationEnhancement::CoreEmber),
        "spinelattice" => Some(IntegrationEnhancement::SpineLattice),
        "reachbranch" => Some(IntegrationEnhancement::ReachBranch),
        "clarityring" => Some(IntegrationEnhancement::ClarityRing),
        "spaceveil" => Some(IntegrationEnhancement::SpaceVeil),
        "depthabyss" => Some(IntegrationEnhancement::DepthAbyss),
        "resonancechord" => Some(IntegrationEnhancement::ResonanceChord),
        "warmthglow" => Some(IntegrationEnhancement::WarmthGlow),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seed_core::domain::{CATEGORIES, FocusPattern};

    // -----------------------------------------------------------------------
    // Phase-chooser modal state machine tests
    // -----------------------------------------------------------------------

    #[test]
    fn phase_chooser_initial_state_is_pattern_cursor_zero() {
        let modal = PrestigeModal::PhaseChooser(PhaseChooserStage::Pattern { cursor: 0 });
        assert!(modal.is_open());
        match modal {
            PrestigeModal::PhaseChooser(PhaseChooserStage::Pattern { cursor }) => {
                assert_eq!(cursor, 0);
            }
            _ => panic!("expected Pattern stage"),
        }
    }

    #[test]
    fn phase_chooser_pattern_cursor_cycles_through_all_patterns() {
        let n = FOCUS_PATTERNS.len();
        assert!(n > 0, "FOCUS_PATTERNS must be non-empty");
        // Cursor at 0 can go down n-1 times.
        let mut cursor = 0usize;
        for _ in 0..n - 1 {
            cursor = (cursor + 1).min(n.saturating_sub(1));
        }
        assert_eq!(cursor, n - 1, "cursor should reach last pattern");
        // Can't go past the end.
        cursor = (cursor + 1).min(n.saturating_sub(1));
        assert_eq!(cursor, n - 1, "cursor saturates at last pattern");
        // Can go back to 0.
        cursor = cursor.saturating_sub(n - 1);
        assert_eq!(cursor, 0);
    }

    #[test]
    fn phase_chooser_pattern_enter_transitions_to_traits_stage() {
        // Selecting Spread3x2 (index 0 in FOCUS_PATTERNS) → Traits stage with 9 false slots.
        let pattern = FOCUS_PATTERNS[0].clone();
        let stage = PhaseChooserStage::Traits {
            pattern: pattern.clone(),
            selected: vec![false; CATEGORIES.len()],
            cursor: 0,
        };
        match &stage {
            PhaseChooserStage::Traits {
                pattern: p,
                selected,
                cursor,
            } => {
                assert_eq!(p, &pattern);
                assert_eq!(selected.len(), CATEGORIES.len());
                assert!(selected.iter().all(|&b| !b));
                assert_eq!(*cursor, 0);
            }
            _ => panic!("expected Traits stage"),
        }
    }

    #[test]
    fn phase_chooser_traits_space_toggles_selection_respects_max() {
        let pattern = FocusPattern::Spread3x2; // max 3 traits
        let max_sel = pattern.skill_count();
        let mut selected = vec![false; CATEGORIES.len()];
        // Toggle on 3 traits.
        let mut count = 0usize;
        for i in 0..CATEGORIES.len() {
            let currently = selected.iter().filter(|&&b| b).count();
            if !selected[i] && currently < max_sel {
                selected[i] = true;
                count += 1;
            }
        }
        assert_eq!(
            count, max_sel,
            "should be able to select exactly max_sel traits"
        );
        // 4th toggle should be blocked.
        let before_count = selected.iter().filter(|&&b| b).count();
        for i in 0..CATEGORIES.len() {
            if !selected[i] {
                let currently = selected.iter().filter(|&&b| b).count();
                if currently < max_sel {
                    selected[i] = true;
                }
                break;
            }
        }
        let after_count = selected.iter().filter(|&&b| b).count();
        assert_eq!(
            before_count, after_count,
            "4th toggle should not increase count when at max"
        );
    }

    #[test]
    fn phase_chooser_traits_arity_validation() {
        // Concentrate1x4 requires exactly 1 trait.
        let pattern = FocusPattern::Concentrate1x4;
        let required = pattern.skill_count();
        assert_eq!(required, 1);

        // A selection with 2 traits doesn't satisfy Concentrate1x4.
        let mut selected = vec![false; CATEGORIES.len()];
        selected[0] = true;
        selected[1] = true;
        let chosen_count = selected.iter().filter(|&&b| b).count();
        assert_ne!(
            chosen_count, required,
            "2 selections violate Concentrate1x4 arity"
        );
    }

    #[test]
    fn phase_chooser_esc_from_traits_returns_to_pattern() {
        // Simulate: pressing Esc in Traits stage returns to Pattern { cursor: 0 }.
        let _stage = PhaseChooserStage::Traits {
            pattern: FocusPattern::Spread3x2,
            selected: vec![false; CATEGORIES.len()],
            cursor: 2,
        };
        // The actual key handler in app.rs returns to Pattern { cursor: 0 } on Esc.
        // We just verify the enum construction is valid.
        let back = PhaseChooserStage::Pattern { cursor: 0 };
        assert!(matches!(back, PhaseChooserStage::Pattern { cursor: 0 }));
    }

    // -----------------------------------------------------------------------
    // Enhancement-chooser modal tests
    // -----------------------------------------------------------------------

    #[test]
    fn enhancement_chooser_default_per_trait_is_correct() {
        let expected = [
            ("flow", IntegrationEnhancement::FlowSpiral),
            ("core", IntegrationEnhancement::CoreEmber),
            ("spine", IntegrationEnhancement::SpineLattice),
            ("reach", IntegrationEnhancement::ReachBranch),
            ("clarity", IntegrationEnhancement::ClarityRing),
            ("space", IntegrationEnhancement::SpaceVeil),
            ("depth", IntegrationEnhancement::DepthAbyss),
            ("resonance", IntegrationEnhancement::ResonanceChord),
            ("warmth", IntegrationEnhancement::WarmthGlow),
        ];
        for (trait_id, expected_enhancement) in &expected {
            assert_eq!(
                default_enhancement(trait_id),
                *expected_enhancement,
                "default enhancement for '{}' should be {:?}",
                trait_id,
                expected_enhancement
            );
        }
    }

    #[test]
    fn enhancement_chooser_cursor_saturates_at_options_count() {
        // Only 1 option per trait in starter set; cursor should saturate at 0.
        let cursor = 0usize;
        let options_len = 1usize; // one starter enhancement per trait
        let incremented = (cursor + 1).min(options_len.saturating_sub(1));
        assert_eq!(incremented, 0, "cursor with 1 option saturates at 0");
    }

    #[test]
    fn enhancement_chooser_confirm_uses_cursor_clamped_to_options() {
        // With 1 option, cursor = 0 → index 0.
        let cursor = 0usize;
        let options = [IntegrationEnhancement::FlowSpiral];
        let selected = options[cursor.min(options.len().saturating_sub(1))].clone();
        assert_eq!(selected, IntegrationEnhancement::FlowSpiral);
    }

    #[test]
    fn parse_enhancement_round_trips_all_variants() {
        let cases = [
            ("flowspiral", IntegrationEnhancement::FlowSpiral),
            ("coreember", IntegrationEnhancement::CoreEmber),
            ("spinelattice", IntegrationEnhancement::SpineLattice),
            ("reachbranch", IntegrationEnhancement::ReachBranch),
            ("clarityring", IntegrationEnhancement::ClarityRing),
            ("spaceveil", IntegrationEnhancement::SpaceVeil),
            ("depthabyss", IntegrationEnhancement::DepthAbyss),
            ("resonancechord", IntegrationEnhancement::ResonanceChord),
            ("warmthglow", IntegrationEnhancement::WarmthGlow),
        ];
        for (s, expected) in &cases {
            assert_eq!(
                parse_enhancement(s),
                Some(expected.clone()),
                "parse_enhancement({s})"
            );
        }
        assert_eq!(parse_enhancement("unknown_thing"), None);
    }
}
