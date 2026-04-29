use chrono::{DateTime, Utc};
use pretty_assertions::assert_eq;
use seed_core::{
    domain::{FocusPattern, IntegrationEnhancement, ReminderId, Tier, TraitId},
    events::{
        Event, EventEnvelope, apply_event, can_activate_phase, from_envelope, to_envelope,
        tokens_available,
    },
    levels::xp_for_level,
    state::initial_state,
};

fn ts() -> DateTime<Utc> {
    "2026-04-22T12:00:00Z".parse().unwrap()
}

fn round_trip(event: Event) -> Event {
    let env = to_envelope(&event, ts());
    let json = serde_json::to_string(&env).unwrap();
    let env2: EventEnvelope = serde_json::from_str(&json).unwrap();
    from_envelope(env2).unwrap()
}

#[test]
fn round_trip_reminder_completed() {
    let e = Event::ReminderCompleted {
        reminder_id: ReminderId("water".into()),
        xp_gained: 74,
        trait_id: TraitId("flow".into()),
        new_xp: 1234,
        streak: 5,
        at_ms: 1_745_000_000_000,
    };
    assert_eq!(round_trip(e.clone()), e);
}

#[test]
fn round_trip_reminder_snoozed() {
    let e = Event::ReminderSnoozed {
        reminder_id: ReminderId("eyes".into()),
        until_ms: 9_999_999,
        snooze_min: 10,
    };
    assert_eq!(round_trip(e.clone()), e);
}

#[test]
fn round_trip_reminder_enabled() {
    let e = Event::ReminderEnabled {
        reminder_id: ReminderId("med".into()),
    };
    assert_eq!(round_trip(e.clone()), e);
}

#[test]
fn round_trip_reminder_disabled() {
    let e = Event::ReminderDisabled {
        reminder_id: ReminderId("med".into()),
    };
    assert_eq!(round_trip(e.clone()), e);
}

#[test]
fn round_trip_reminder_pinned() {
    let e = Event::ReminderPinned {
        reminder_id: ReminderId("walk".into()),
    };
    assert_eq!(round_trip(e.clone()), e);
}

#[test]
fn round_trip_reminder_unpinned() {
    let e = Event::ReminderUnpinned {
        reminder_id: ReminderId("walk".into()),
    };
    assert_eq!(round_trip(e.clone()), e);
}

#[test]
fn round_trip_trait_xp_changed() {
    let e = Event::TraitXpChanged {
        trait_id: TraitId("core".into()),
        delta: 55,
        new_xp: 500,
    };
    assert_eq!(round_trip(e.clone()), e);
}

#[test]
fn round_trip_level_up() {
    let e = Event::LevelUp {
        trait_id: TraitId("reach".into()),
        old_level: 4,
        new_level: 5,
        new_xp: 1154,
    };
    assert_eq!(round_trip(e.clone()), e);
}

#[test]
fn round_trip_tier_changed() {
    let e = Event::TierChanged {
        from: Tier::Seed,
        to: Tier::Sprout,
        total_level: 20,
    };
    assert_eq!(round_trip(e.clone()), e);
}

#[test]
fn round_trip_companion_awakened() {
    let e = Event::CompanionAwakened { glyph_seed: 42 };
    assert_eq!(round_trip(e.clone()), e);
}

#[test]
fn round_trip_config_changed() {
    let e = Event::ConfigChanged {
        key: "palette".to_string(),
        value: serde_json::json!("dusk"),
    };
    assert_eq!(round_trip(e.clone()), e);
}

/// Unknown event round-trips without data loss (Fix 1).
#[test]
fn round_trip_unknown_preserves_kind_and_data() {
    let e = Event::Unknown {
        kind: "seed.future.thing".to_string(),
        data: serde_json::json!({"x": 42}),
    };
    assert_eq!(round_trip(e.clone()), e);
}

/// Schema lock: byte-equal JSON snapshot for ReminderCompleted.
/// Rename any field → this test breaks, protecting pour integration.
#[test]
fn snapshot_reminder_completed_json_shape() {
    let event = Event::ReminderCompleted {
        reminder_id: ReminderId("water".into()),
        xp_gained: 74,
        trait_id: TraitId("flow".into()),
        new_xp: 1234,
        streak: 5,
        at_ms: 1_745_000_000_000,
    };
    let env = to_envelope(&event, ts());
    let json = serde_json::to_string(&env).unwrap();

    // Schema lock: serde_json serialises Map keys in sorted order when going
    // through Value. Any field rename changes this string and breaks the test.
    // at_ms added in Wave 3.1 to stamp last_done_ms on completion.
    let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.reminder.completed","data":{"at_ms":1745000000000,"new_xp":1234,"reminder_id":"water","streak":5,"trait_id":"flow","xp_gained":74}}"#;
    assert_eq!(json, expected);
}

#[test]
fn envelope_v_field_is_1() {
    let e = Event::CompanionAwakened { glyph_seed: 1 };
    let env = to_envelope(&e, ts());
    assert_eq!(env.v, 1);
}

#[test]
fn all_kinds_are_seed_namespaced() {
    let events = vec![
        Event::ReminderCompleted {
            reminder_id: ReminderId("water".into()),
            xp_gained: 55,
            trait_id: TraitId("flow".into()),
            new_xp: 0,
            streak: 0,
            at_ms: 0,
        },
        Event::ReminderSnoozed {
            reminder_id: ReminderId("eyes".into()),
            until_ms: 0,
            snooze_min: 10,
        },
        Event::ReminderEnabled {
            reminder_id: ReminderId("med".into()),
        },
        Event::ReminderDisabled {
            reminder_id: ReminderId("med".into()),
        },
        Event::ReminderPinned {
            reminder_id: ReminderId("walk".into()),
        },
        Event::ReminderUnpinned {
            reminder_id: ReminderId("walk".into()),
        },
        Event::TraitXpChanged {
            trait_id: TraitId("core".into()),
            delta: 0,
            new_xp: 0,
        },
        Event::LevelUp {
            trait_id: TraitId("core".into()),
            old_level: 1,
            new_level: 2,
            new_xp: 83,
        },
        Event::TierChanged {
            from: Tier::Seed,
            to: Tier::Sprout,
            total_level: 18,
        },
        Event::CompanionAwakened { glyph_seed: 42 },
        Event::ConfigChanged {
            key: "palette".to_string(),
            value: serde_json::json!("sage"),
        },
    ];
    for event in events {
        let env = to_envelope(&event, ts());
        assert!(
            env.kind.starts_with("seed."),
            "kind {:?} is not seed-namespaced",
            env.kind
        );
    }
}

// ---------------------------------------------------------------------------
// New event variant round-trips (Stage G)
// ---------------------------------------------------------------------------

#[test]
fn round_trip_trait_integrated() {
    let e = Event::TraitIntegrated {
        trait_id: TraitId("flow".into()),
        new_integrations: 1,
        enhancement_id: IntegrationEnhancement::FlowSpiral,
    };
    let rt = round_trip(e.clone());
    assert_eq!(rt, e);
    // Verify kind string.
    let env = to_envelope(&e, ts());
    assert_eq!(env.kind, "seed.trait.integrated");
}

#[test]
fn round_trip_focus_token_earned() {
    let e = Event::FocusTokenEarned { new_balance: 2 };
    let rt = round_trip(e.clone());
    assert_eq!(rt, e);
    let env = to_envelope(&e, ts());
    assert_eq!(env.kind, "seed.focus.token_earned");
}

#[test]
fn round_trip_focus_phase_activated() {
    let e = Event::FocusPhaseActivated {
        pattern: FocusPattern::Spread3x2,
        traits: vec![
            TraitId("flow".into()),
            TraitId("core".into()),
            TraitId("spine".into()),
        ],
    };
    let rt = round_trip(e.clone());
    assert_eq!(rt, e);
    let env = to_envelope(&e, ts());
    assert_eq!(env.kind, "seed.focus.phase_activated");
}

// ---------------------------------------------------------------------------
// TraitIntegrated fold (Stage G)
// ---------------------------------------------------------------------------

const NOW_MS: i64 = 1_745_000_000_000;

#[test]
fn integrate_fold_resets_xp_and_increments_count() {
    let mut state = initial_state(NOW_MS);

    // Manually set flow XP to level 99.
    let xp99 = xp_for_level(99);
    *state.traits.get_mut(&TraitId("flow".into())).unwrap() = xp99;

    apply_event(
        &mut state,
        &Event::TraitIntegrated {
            trait_id: TraitId("flow".into()),
            new_integrations: 1,
            enhancement_id: IntegrationEnhancement::FlowSpiral,
        },
    );

    // XP must reset to 0.
    assert_eq!(*state.traits.get(&TraitId("flow".into())).unwrap(), 0);
    // Integrations incremented.
    assert_eq!(
        *state
            .trait_integrations
            .get(&TraitId("flow".into()))
            .unwrap_or(&0),
        1
    );
    // Enhancement appended.
    let enhancements = state
        .trait_enhancements
        .get(&TraitId("flow".into()))
        .unwrap();
    assert_eq!(enhancements.len(), 1);
    assert_eq!(enhancements[0], IntegrationEnhancement::FlowSpiral);
}

#[test]
fn integrate_fold_second_time_stacks_enhancements() {
    let mut state = initial_state(NOW_MS);
    let xp99 = xp_for_level(99);

    // First integration.
    *state.traits.get_mut(&TraitId("flow".into())).unwrap() = xp99;
    apply_event(
        &mut state,
        &Event::TraitIntegrated {
            trait_id: TraitId("flow".into()),
            new_integrations: 1,
            enhancement_id: IntegrationEnhancement::FlowSpiral,
        },
    );

    // Second integration — must set XP to 99 again first.
    *state.traits.get_mut(&TraitId("flow".into())).unwrap() = xp99;
    apply_event(
        &mut state,
        &Event::TraitIntegrated {
            trait_id: TraitId("flow".into()),
            new_integrations: 2,
            enhancement_id: IntegrationEnhancement::FlowSpiral,
        },
    );

    assert_eq!(
        *state
            .trait_integrations
            .get(&TraitId("flow".into()))
            .unwrap_or(&0),
        2
    );
    let enhancements = state
        .trait_enhancements
        .get(&TraitId("flow".into()))
        .unwrap();
    assert_eq!(enhancements.len(), 2);
}

#[test]
fn integrate_fold_ignored_when_not_at_99() {
    let mut state = initial_state(NOW_MS);
    // Set XP to level 50 (not 99).
    *state.traits.get_mut(&TraitId("flow".into())).unwrap() = xp_for_level(50);

    apply_event(
        &mut state,
        &Event::TraitIntegrated {
            trait_id: TraitId("flow".into()),
            new_integrations: 1,
            enhancement_id: IntegrationEnhancement::FlowSpiral,
        },
    );

    // State must be unchanged.
    let xp = *state.traits.get(&TraitId("flow".into())).unwrap();
    assert_eq!(
        xp,
        xp_for_level(50),
        "XP should not change for non-99 trait"
    );
    assert_eq!(
        *state
            .trait_integrations
            .get(&TraitId("flow".into()))
            .unwrap_or(&0),
        0
    );
    assert!(
        !state
            .trait_enhancements
            .contains_key(&TraitId("flow".into()))
    );
}

// ---------------------------------------------------------------------------
// Focus token earning fold (Stage G / C1 fix)
// ---------------------------------------------------------------------------

#[test]
fn level_up_fold_increments_cumulative_levels() {
    let mut state = initial_state(NOW_MS);
    assert_eq!(state.cumulative_levels_gained, 0);

    // old_level is carried in the payload (C1 fix). The fold must use it, NOT
    // read from state.traits (which may already hold new_xp if ReminderCompleted
    // ran first in the same commit batch — the production path).
    apply_event(
        &mut state,
        &Event::LevelUp {
            trait_id: TraitId("flow".into()),
            old_level: 1,
            new_level: 2,
            new_xp: xp_for_level(2),
        },
    );

    assert_eq!(state.cumulative_levels_gained, 1);
}

/// Production-path test for C1: fold must correctly increment cumulative_levels_gained
/// even when state.traits already holds new_xp (as it does after ReminderCompleted
/// runs first in the daemon's commit batch). The key invariant is that the fold reads
/// old_level from the payload, not from current state.
#[test]
fn level_up_fold_uses_payload_old_level_not_current_state() {
    let mut state = initial_state(NOW_MS);

    // Simulate the production path: set flow XP to level 2 BEFORE applying LevelUp
    // (as if ReminderCompleted already ran and overwrote state.traits).
    *state.traits.get_mut(&TraitId("flow".into())).unwrap() = xp_for_level(2);

    // Now apply LevelUp with old_level=1, new_level=2.
    // If the fold reads from state (old bug), level_for_xp(xp_for_level(2)) == 2 == new_level,
    // so levels_gained = 0 and cumulative_levels_gained stays 0.
    // With the fix, old_level=1 from payload → levels_gained = 1.
    apply_event(
        &mut state,
        &Event::LevelUp {
            trait_id: TraitId("flow".into()),
            old_level: 1,
            new_level: 2,
            new_xp: xp_for_level(2),
        },
    );

    assert_eq!(
        state.cumulative_levels_gained, 1,
        "fold must use payload old_level, not current state (C1 fix)"
    );
}

/// Feed enough LevelUp events to cross a 99-multiple boundary.
/// Replay must produce the same cumulative_levels_gained deterministically.
/// Both passes use the production path — no manual state pre-setting (C1 fix).
#[test]
fn replay_cumulative_levels_deterministic() {
    // Build the event list once (with old_level in each payload).
    let events: Vec<Event> = (0u32..100)
        .map(|i| {
            let trait_name = match i % 9 {
                0 => "flow",
                1 => "core",
                2 => "spine",
                3 => "reach",
                4 => "clarity",
                5 => "space",
                6 => "depth",
                7 => "resonance",
                _ => "warmth",
            };
            let old_level = ((i % 98) + 1) as u8; // 1-based: level before gain
            let new_level = ((i % 98) + 2) as u8;
            Event::LevelUp {
                trait_id: TraitId(trait_name.into()),
                old_level,
                new_level,
                new_xp: xp_for_level(new_level),
            }
        })
        .collect();

    // First pass.
    let mut state = initial_state(NOW_MS);
    for ev in &events {
        apply_event(&mut state, ev);
    }

    // We gained 100 levels total → crossed the 99-level boundary once.
    assert_eq!(state.cumulative_levels_gained, 100);
    // tokens_available = 100 / 99 - 0 = 1.
    assert_eq!(tokens_available(&state), 1);

    // Replay from scratch must produce identical total — no pre-setting of state.
    let mut state2 = initial_state(NOW_MS);
    for ev in &events {
        apply_event(&mut state2, ev);
    }
    assert_eq!(
        state2.cumulative_levels_gained, state.cumulative_levels_gained,
        "replay must be deterministic"
    );
}

// ---------------------------------------------------------------------------
// FocusPhaseActivated fold (Stage G)
// ---------------------------------------------------------------------------

#[test]
fn focus_phase_activated_sets_active_focus() {
    let mut state = initial_state(NOW_MS);
    // Grant a token by setting cumulative_levels_gained ≥ 99.
    state.cumulative_levels_gained = 99;
    assert_eq!(tokens_available(&state), 1);
    assert!(can_activate_phase(&state));

    apply_event(
        &mut state,
        &Event::FocusPhaseActivated {
            pattern: FocusPattern::Spread3x2,
            traits: vec![
                TraitId("flow".into()),
                TraitId("core".into()),
                TraitId("spine".into()),
            ],
        },
    );

    assert_eq!(state.tokens_spent, 1);
    assert_eq!(tokens_available(&state), 0);
    assert!(!can_activate_phase(&state));

    let focus = state.active_focus.as_ref().unwrap();
    assert_eq!(focus.pattern, FocusPattern::Spread3x2);
    assert_eq!(focus.allocations.len(), 3);
    // Spread3x2 → 1 arrow per trait.
    for (_, arrows) in &focus.allocations {
        assert_eq!(*arrows, 1);
    }
}

#[test]
fn focus_phase_activated_ignored_with_no_tokens() {
    let mut state = initial_state(NOW_MS);
    // No tokens available.
    assert_eq!(tokens_available(&state), 0);

    apply_event(
        &mut state,
        &Event::FocusPhaseActivated {
            pattern: FocusPattern::Spread3x2,
            traits: vec![
                TraitId("flow".into()),
                TraitId("core".into()),
                TraitId("spine".into()),
            ],
        },
    );

    assert_eq!(state.tokens_spent, 0);
    assert!(state.active_focus.is_none());
}

#[test]
fn focus_phase_activated_rejected_wrong_trait_count() {
    let mut state = initial_state(NOW_MS);
    state.cumulative_levels_gained = 99; // give 1 token

    // Spread3x2 expects 3 traits, but we only pass 1.
    apply_event(
        &mut state,
        &Event::FocusPhaseActivated {
            pattern: FocusPattern::Spread3x2,
            traits: vec![TraitId("flow".into())],
        },
    );

    // Token must not be spent; focus must not be set.
    assert_eq!(state.tokens_spent, 0);
    assert!(state.active_focus.is_none());
}

// ---------------------------------------------------------------------------
// Snapshot round-trip (Stage G)
// ---------------------------------------------------------------------------

#[test]
fn state_snapshot_round_trips_new_fields() {
    let mut state = initial_state(NOW_MS);
    let xp99 = xp_for_level(99);

    // Set up some prestige state.
    *state.traits.get_mut(&TraitId("flow".into())).unwrap() = xp99;
    apply_event(
        &mut state,
        &Event::TraitIntegrated {
            trait_id: TraitId("flow".into()),
            new_integrations: 1,
            enhancement_id: IntegrationEnhancement::FlowSpiral,
        },
    );

    state.cumulative_levels_gained = 99;
    apply_event(
        &mut state,
        &Event::FocusPhaseActivated {
            pattern: FocusPattern::Concentrate1x4,
            traits: vec![TraitId("core".into())],
        },
    );

    let json = serde_json::to_string(&state).unwrap();
    let state2: seed_core::state::State = serde_json::from_str(&json).unwrap();

    assert_eq!(
        state2
            .trait_integrations
            .get(&TraitId("flow".into()))
            .copied()
            .unwrap_or(0),
        1
    );
    assert_eq!(
        state2
            .trait_enhancements
            .get(&TraitId("flow".into()))
            .map(|v| v.len())
            .unwrap_or(0),
        1
    );
    assert_eq!(state2.cumulative_levels_gained, 99);
    assert_eq!(state2.tokens_spent, 1);
    assert!(state2.active_focus.is_some());
    let focus = state2.active_focus.unwrap();
    assert_eq!(focus.pattern, FocusPattern::Concentrate1x4);
    assert_eq!(focus.allocations.len(), 1);
    assert_eq!(focus.allocations[0].1, 3); // Concentrate1x4 → 3 arrows
}

/// Old snapshots (without new fields) must still deserialize cleanly.
#[test]
fn old_snapshot_without_prestige_fields_deserializes() {
    // Simulate an old snapshot JSON that lacks the new fields.
    let old_json = r#"{
        "awakened_at": 1745000000000,
        "traits": {"flow": 0, "core": 0, "spine": 0, "reach": 0, "clarity": 0,
                   "space": 0, "depth": 0, "resonance": 0, "warmth": 0},
        "reminders": {},
        "glyph_seed": 42,
        "active_hours": [7, 22],
        "snooze_min": 5,
        "notif_style": "flash",
        "palette": "sage",
        "log": [],
        "last_tick_ms": 1745000000000,
        "completed_total": 0,
        "missed_total": 0,
        "xp_multiplier": 1
    }"#;

    let state: seed_core::state::State = serde_json::from_str(old_json).unwrap();
    // New fields must default cleanly.
    assert_eq!(state.cumulative_levels_gained, 0);
    assert_eq!(state.tokens_spent, 0);
    assert!(state.active_focus.is_none());
    assert!(state.trait_integrations.is_empty());
    assert!(state.trait_enhancements.is_empty());
}

// ---------------------------------------------------------------------------
// C3 fix: TraitIntegrated increments, not overwrites
// ---------------------------------------------------------------------------

/// Folding TraitIntegrated twice must yield integrations == 2, not 1.
/// Old bug: *count = *new_integrations — trusting payload verbatim made the
/// fold non-idempotent (two events with new_integrations=2 would give 2,
/// not 3). Fix: always saturating_add(1).
#[test]
fn integrate_twice_increments_to_two() {
    let mut state = initial_state(NOW_MS);
    let xp99 = xp_for_level(99);

    // First integration.
    *state.traits.get_mut(&TraitId("flow".into())).unwrap() = xp99;
    apply_event(
        &mut state,
        &Event::TraitIntegrated {
            trait_id: TraitId("flow".into()),
            new_integrations: 1,
            enhancement_id: IntegrationEnhancement::FlowSpiral,
        },
    );
    assert_eq!(
        *state
            .trait_integrations
            .get(&TraitId("flow".into()))
            .unwrap_or(&0),
        1,
        "after first integrate: count must be 1"
    );

    // Second integration.
    *state.traits.get_mut(&TraitId("flow".into())).unwrap() = xp99;
    apply_event(
        &mut state,
        &Event::TraitIntegrated {
            trait_id: TraitId("flow".into()),
            new_integrations: 2,
            enhancement_id: IntegrationEnhancement::FlowSpiral,
        },
    );
    assert_eq!(
        *state
            .trait_integrations
            .get(&TraitId("flow".into()))
            .unwrap_or(&0),
        2,
        "after second integrate: count must be 2 (not payload value)"
    );
    assert_eq!(
        state
            .trait_enhancements
            .get(&TraitId("flow".into()))
            .unwrap()
            .len(),
        2,
        "two enhancements must be appended"
    );
}

// ---------------------------------------------------------------------------
// m8: FocusPhaseActivated rejects duplicate trait IDs
// ---------------------------------------------------------------------------

#[test]
fn focus_phase_activated_rejected_duplicate_trait_ids() {
    let mut state = initial_state(NOW_MS);
    state.cumulative_levels_gained = 99; // give 1 token

    // Spread3x2 expects 3 traits, but we pass duplicates.
    apply_event(
        &mut state,
        &Event::FocusPhaseActivated {
            pattern: FocusPattern::Spread3x2,
            traits: vec![
                TraitId("flow".into()),
                TraitId("flow".into()),
                TraitId("flow".into()),
            ],
        },
    );

    // Token must not be spent; focus must not be set.
    assert_eq!(
        state.tokens_spent, 0,
        "duplicate traits must not spend a token"
    );
    assert!(
        state.active_focus.is_none(),
        "duplicate traits must not set active_focus"
    );
}

// ---------------------------------------------------------------------------
// m9: FocusPhaseActivated rejects unknown trait IDs
// ---------------------------------------------------------------------------

#[test]
fn focus_phase_activated_rejected_unknown_trait_id() {
    let mut state = initial_state(NOW_MS);
    state.cumulative_levels_gained = 99; // give 1 token

    // One of the traits doesn't exist in state.
    apply_event(
        &mut state,
        &Event::FocusPhaseActivated {
            pattern: FocusPattern::Spread3x2,
            traits: vec![
                TraitId("flow".into()),
                TraitId("core".into()),
                TraitId("phantom_trait".into()), // does not exist
            ],
        },
    );

    // Token must not be spent; focus must not be set.
    assert_eq!(
        state.tokens_spent, 0,
        "unknown trait must not spend a token"
    );
    assert!(
        state.active_focus.is_none(),
        "unknown trait must not set active_focus"
    );
}
