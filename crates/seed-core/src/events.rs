/// Append-only event log types. Every mutation to State is expressed as an Event.
///
/// Wire format: `{ "v": 1, "ts": "<rfc3339>", "kind": "seed.<noun>.<verb>", "data": { ... } }`
/// All `kind` strings are namespaced under `seed.*` for pour-integration forward-compat.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    domain::{
        CATEGORIES, FocusPattern, FocusPhase, IntegrationEnhancement, REMINDERS, ReminderId, Tier,
        TraitId,
    },
    levels::xp_for_level,
    state::{LogEntry, State, TraitSkipStats},
};

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub v: u32,
    pub ts: DateTime<Utc>,
    pub kind: String,
    pub data: Value,
}

// ---------------------------------------------------------------------------
// Event enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Event {
    #[serde(rename = "seed.reminder.completed")]
    ReminderCompleted {
        reminder_id: ReminderId,
        xp_gained: u32,
        trait_id: TraitId,
        new_xp: u64,
        streak: u32,
        /// Unix timestamp (ms) when the completion was recorded.
        /// Used by `apply_event` to stamp `last_done_ms` on the reminder runtime,
        /// which resets the scheduler's due/overdue state and stops XP drain.
        at_ms: i64,
    },
    #[serde(rename = "seed.reminder.snoozed")]
    ReminderSnoozed {
        reminder_id: ReminderId,
        until_ms: i64,
        snooze_min: u32,
    },
    #[serde(rename = "seed.reminder.enabled")]
    ReminderEnabled { reminder_id: ReminderId },
    #[serde(rename = "seed.reminder.disabled")]
    ReminderDisabled { reminder_id: ReminderId },
    #[serde(rename = "seed.reminder.pinned")]
    ReminderPinned { reminder_id: ReminderId },
    #[serde(rename = "seed.reminder.unpinned")]
    ReminderUnpinned { reminder_id: ReminderId },
    #[serde(rename = "seed.trait.xp_changed")]
    TraitXpChanged {
        trait_id: TraitId,
        delta: i64,
        new_xp: u64,
    },
    #[serde(rename = "seed.trait.level_up")]
    LevelUp {
        trait_id: TraitId,
        /// Level before this event was applied. Carried in the payload so the
        /// fold can compute `levels_gained = new_level - old_level` without
        /// reading pre-event state (which may already be overwritten by a
        /// preceding `ReminderCompleted` in the same commit batch).
        ///
        /// Serde default = 0 so old log entries without this field round-trip
        /// cleanly; the fold treats old_level=0 as a 1-level gain (safe
        /// undercount for legacy events, not a loss).
        #[serde(default)]
        old_level: u8,
        new_level: u8,
        new_xp: u64,
    },
    #[serde(rename = "seed.companion.tier_changed")]
    TierChanged {
        from: Tier,
        to: Tier,
        total_level: u32,
    },
    #[serde(rename = "seed.companion.awakened")]
    CompanionAwakened { glyph_seed: u64 },
    #[serde(rename = "seed.config.changed")]
    ConfigChanged { key: String, value: Value },
    /// Tracks the last time a reminder notification was fired (debounce marker).
    /// `at_ms` is the Unix timestamp in milliseconds when the notification fired.
    #[serde(rename = "seed.reminder.notified")]
    ReminderNotified { reminder_id: ReminderId, at_ms: i64 },
    /// Daemon auto-skipped an overdue reminder that exceeded 2× its interval.
    /// Resets `last_done_ms` so the reminder returns to Dormant. When
    /// `was_snoozed` is false, also zeroes `streak` and increments `total_missed`.
    /// Emitted regardless of active hours — state cleanup must run during sleep
    /// so the user wakes to a clean board.
    #[serde(rename = "seed.reminder.skipped")]
    ReminderSkipped {
        /// Alphabetical field order matches serde_json output for the snapshot test.
        at_ms: i64,
        missed_cycles: u32,
        reminder_id: ReminderId,
        /// True when the user snoozed at some point during this overdue cycle
        /// (`snoozed_until_ms > last_done_ms`). Suppresses streak reset and
        /// `total_missed` increment — the user signalled intent, so we coax
        /// rather than penalise.
        #[serde(default)]
        was_snoozed: bool,
    },
    /// Override the cadence for one reminder. Persisted via event log; replayed
    /// by `apply_event` into `ReminderRuntime::interval_min`.
    #[serde(rename = "seed.reminder.interval_changed")]
    ReminderIntervalChanged {
        reminder_id: ReminderId,
        minutes: u32,
    },
    /// A trait was integrated (reset from lvl 99 to 1) and the user chose
    /// a visual enhancement. Emitted at user request; `apply_event` validates
    /// the trait is at lvl 99 and ignores the event otherwise (defensive).
    #[serde(rename = "seed.trait.integrated")]
    TraitIntegrated {
        trait_id: TraitId,
        /// New cumulative integration count for this trait (1-based).
        new_integrations: u8,
        /// The visual enhancement chosen at this integration.
        enhancement_id: IntegrationEnhancement,
    },
    /// Emitted inside the `apply_event` fold for `LevelUp` whenever the
    /// increment to `cumulative_levels_gained` crosses a 99-multiple boundary.
    /// Computed deterministically so replays produce identical token totals.
    #[serde(rename = "seed.focus.token_earned")]
    FocusTokenEarned {
        /// Available token balance after this award (`cumulative_levels_gained / 99 - tokens_spent`).
        new_balance: u32,
    },
    /// User spent a focus token to activate a phase. `apply_event` validates
    /// `traits.len()` matches the pattern and `tokens_available > 0`.
    #[serde(rename = "seed.focus.phase_activated")]
    FocusPhaseActivated {
        pattern: FocusPattern,
        /// Trait IDs to allocate — exactly 1, 2, or 3 depending on `pattern`.
        traits: Vec<TraitId>,
    },
    /// Forward-compat fallback: unknown future events deserialize here,
    /// preserving the original `kind` string and entire `data` payload.
    /// This prevents data loss when a newer client writes events that an older
    /// client replays — the envelope is preserved verbatim.
    Unknown { kind: String, data: Value },
}

// ---------------------------------------------------------------------------
// Envelope conversion
// ---------------------------------------------------------------------------

/// Serialize `event` into a wire envelope with the given timestamp.
pub fn to_envelope(event: &Event, ts: DateTime<Utc>) -> EventEnvelope {
    match event {
        Event::Unknown { kind, data } => EventEnvelope {
            v: 1,
            ts,
            kind: kind.clone(),
            data: data.clone(),
        },
        _ => {
            let kind = event_kind(event);
            // Serialize through Value to extract the `data` field.
            let data = serde_json::to_value(event)
                .ok()
                .and_then(|v| {
                    if let Value::Object(mut m) = v {
                        m.remove("data")
                    } else {
                        None
                    }
                })
                .unwrap_or(Value::Null);

            EventEnvelope {
                v: 1,
                ts,
                kind: kind.to_string(),
                data,
            }
        }
    }
}

/// Reconstruct an `Event` from a wire envelope.
///
/// For known `kind` strings, attempts full deserialization of the `data` payload.
/// If the `kind` is known but `data` fails to decode (schema mismatch), returns
/// `Err` so callers can distinguish "unknown kind" from "corrupt known event".
/// For unrecognised kinds, returns `Event::Unknown { kind, data }` preserving
/// the original payload for forward-compat round-trips.
pub fn from_envelope(env: EventEnvelope) -> Result<Event, serde_json::Error> {
    // Check if this is a known kind by attempting to match against the known set.
    if is_known_kind(&env.kind) {
        let json = serde_json::json!({
            "kind": env.kind,
            "data": env.data,
        });
        serde_json::from_value(json)
    } else {
        Ok(Event::Unknown {
            kind: env.kind,
            data: env.data,
        })
    }
}

/// Returns true if the kind string matches one of the known event variants.
fn is_known_kind(kind: &str) -> bool {
    matches!(
        kind,
        "seed.reminder.completed"
            | "seed.reminder.snoozed"
            | "seed.reminder.enabled"
            | "seed.reminder.disabled"
            | "seed.reminder.pinned"
            | "seed.reminder.unpinned"
            | "seed.trait.xp_changed"
            | "seed.trait.level_up"
            | "seed.companion.tier_changed"
            | "seed.companion.awakened"
            | "seed.config.changed"
            | "seed.reminder.notified"
            | "seed.reminder.skipped"
            | "seed.reminder.interval_changed"
            | "seed.trait.integrated"
            | "seed.focus.token_earned"
            | "seed.focus.phase_activated"
    )
}

fn event_kind(event: &Event) -> &'static str {
    match event {
        Event::ReminderCompleted { .. } => "seed.reminder.completed",
        Event::ReminderSnoozed { .. } => "seed.reminder.snoozed",
        Event::ReminderEnabled { .. } => "seed.reminder.enabled",
        Event::ReminderDisabled { .. } => "seed.reminder.disabled",
        Event::ReminderPinned { .. } => "seed.reminder.pinned",
        Event::ReminderUnpinned { .. } => "seed.reminder.unpinned",
        Event::TraitXpChanged { .. } => "seed.trait.xp_changed",
        Event::LevelUp { .. } => "seed.trait.level_up",
        Event::TierChanged { .. } => "seed.companion.tier_changed",
        Event::CompanionAwakened { .. } => "seed.companion.awakened",
        Event::ConfigChanged { .. } => "seed.config.changed",
        Event::ReminderNotified { .. } => "seed.reminder.notified",
        Event::ReminderSkipped { .. } => "seed.reminder.skipped",
        Event::ReminderIntervalChanged { .. } => "seed.reminder.interval_changed",
        Event::TraitIntegrated { .. } => "seed.trait.integrated",
        Event::FocusTokenEarned { .. } => "seed.focus.token_earned",
        Event::FocusPhaseActivated { .. } => "seed.focus.phase_activated",
        // Unknown is handled before event_kind is called in to_envelope.
        Event::Unknown { kind, .. } => {
            // Should not reach here via to_envelope, but be safe.
            let _ = kind;
            "seed.unknown"
        }
    }
}

// ---------------------------------------------------------------------------
// State fold
// ---------------------------------------------------------------------------

/// Apply a single event to state in-place. Single source of truth for all mutations.
///
/// Validation rules:
/// - `ReminderCompleted`: if `reminder_id` not in state.reminders OR `trait_id` not in
///   state.traits, the event is a no-op (no counter increment, no log entry).
/// - All counter increments use `saturating_add` to prevent overflow panics.
pub fn apply_event(state: &mut State, event: &Event) {
    match event {
        Event::ReminderCompleted {
            reminder_id,
            xp_gained,
            trait_id,
            new_xp,
            streak,
            at_ms,
        } => {
            // Validate both ids exist before mutating any state.
            let reminder_known = state.reminders.contains_key(reminder_id);
            let trait_known = state.traits.contains_key(trait_id);

            if !reminder_known || !trait_known {
                // Unknown id — no-op. Callers that need observability should
                // check the ids before calling apply_event and route to their
                // own tracing/logging.
                return;
            }

            if let Some(rt) = state.reminders.get_mut(reminder_id) {
                rt.total_done = rt.total_done.saturating_add(1);
                rt.streak = *streak;
                // Stamp last_done_ms so the scheduler sees the reminder as
                // Dormant and stops XP drain. This is the fix for the
                // "unbounded XP drain after Complete" bug (Wave 3.1 Fix 1).
                rt.last_done_ms = *at_ms;
            }
            if let Some(xp) = state.traits.get_mut(trait_id) {
                *xp = *new_xp;
            }
            state.completed_total = state.completed_total.saturating_add(1);
            state.log.push_back(LogEntry {
                t: String::new(),
                msg: format!("{} · logged. +{} xp", reminder_id.0, xp_gained),
                tag: "accent".to_string(),
            });
        }
        Event::ReminderSnoozed {
            reminder_id,
            until_ms,
            ..
        } => {
            if let Some(rt) = state.reminders.get_mut(reminder_id) {
                rt.snoozed_until_ms = *until_ms;
            }
        }
        Event::ReminderEnabled { reminder_id } => {
            if let Some(rt) = state.reminders.get_mut(reminder_id) {
                rt.enabled = true;
            }
        }
        Event::ReminderDisabled { reminder_id } => {
            if let Some(rt) = state.reminders.get_mut(reminder_id) {
                rt.enabled = false;
            }
        }
        Event::ReminderPinned { reminder_id } => {
            if let Some(rt) = state.reminders.get_mut(reminder_id) {
                rt.pinned = true;
            }
        }
        Event::ReminderUnpinned { reminder_id } => {
            if let Some(rt) = state.reminders.get_mut(reminder_id) {
                rt.pinned = false;
            }
        }
        Event::TraitXpChanged {
            trait_id, new_xp, ..
        } => {
            if let Some(xp) = state.traits.get_mut(trait_id) {
                *xp = *new_xp;
            }
        }
        Event::LevelUp {
            trait_id,
            old_level,
            new_level,
            new_xp,
        } => {
            if let Some(xp) = state.traits.get_mut(trait_id) {
                // `old_level` is carried in the payload so this fold remains
                // correct regardless of what ReminderCompleted already wrote
                // into state.traits before this event was applied. If old_level
                // is missing (legacy log entry, serde default = 0) we fall back
                // to a safe 1-level gain — a minor undercount for ancient events,
                // not a loss.
                let effective_old = if *old_level == 0 {
                    new_level.saturating_sub(1)
                } else {
                    *old_level
                };
                let levels_gained = (*new_level).saturating_sub(effective_old) as u32;
                *xp = *new_xp;

                // Track cumulative levels for focus token earning.
                state.cumulative_levels_gained =
                    state.cumulative_levels_gained.saturating_add(levels_gained);
            }
        }
        Event::ReminderNotified { reminder_id, at_ms } => {
            if let Some(rt) = state.reminders.get_mut(reminder_id) {
                rt.last_notified_ms = *at_ms;
            }
        }
        Event::ReminderSkipped {
            reminder_id,
            at_ms,
            was_snoozed,
            ..
        } => {
            if let Some(rt) = state.reminders.get_mut(reminder_id) {
                // Always roll last_done_ms forward — state cleanup is unconditional.
                rt.last_done_ms = *at_ms;
                if !*was_snoozed {
                    rt.streak = 0;
                    rt.total_missed = rt.total_missed.saturating_add(1);
                }
            }
            if !*was_snoozed {
                // Per-trait skip aggregation: walk catalog to find reminder → category → trait.
                if let Some(reminder) = REMINDERS.iter().find(|r| r.reminder_id() == *reminder_id)
                    && let Some(category) = CATEGORIES.iter().find(|c| c.id == reminder.cat)
                {
                    let trait_id = TraitId(category.trait_id.to_owned());
                    let stats: &mut TraitSkipStats =
                        state.traits_skipped.entry(trait_id).or_default();
                    stats.lifetime = stats.lifetime.saturating_add(1);
                    stats.recent.push(*at_ms);
                    // Prune entries older than 7 days so the vec stays bounded.
                    let cutoff = *at_ms - 7 * 86_400 * 1000;
                    stats.recent.retain(|&ts| ts >= cutoff);
                }
            }
        }
        Event::ReminderIntervalChanged {
            reminder_id,
            minutes,
        } => {
            if let Some(rt) = state.reminders.get_mut(reminder_id) {
                rt.interval_min = *minutes;
            }
        }
        Event::TraitIntegrated {
            trait_id,
            new_integrations,
            enhancement_id,
        } => {
            // Defensive: only apply if the trait is currently at lvl 99.
            let current_xp = state.traits.get(trait_id).copied().unwrap_or(0);
            let current_level = xp_for_level(crate::levels::MAX_LEVEL);
            if current_xp < current_level {
                // Warn without depending on `tracing` (seed-core is pure; callers
                // with tracing can check return conditions before calling apply_event).
                #[cfg(debug_assertions)]
                eprintln!(
                    "[seed-core] TraitIntegrated ignored — trait '{}' is not at \
                     level 99 (xp={current_xp}, need={current_level})",
                    trait_id.0
                );
                return;
            }
            // Reset XP to 0 (level → 1).
            if let Some(xp) = state.traits.get_mut(trait_id) {
                *xp = 0;
            }
            // Increment integration count. The fold always increments by 1;
            // `new_integrations` in the payload is audit-trail-only and is NOT
            // used to set the count (trusting it verbatim would make replay
            // non-idempotent if the payload were wrong).
            let count = state
                .trait_integrations
                .entry(trait_id.clone())
                .or_insert(0);
            #[cfg(debug_assertions)]
            if *count + 1 != *new_integrations {
                eprintln!(
                    "[seed-core] TraitIntegrated payload mismatch: \
                     state count={count}, payload new_integrations={new_integrations}"
                );
            }
            *count = count.saturating_add(1);
            // Append the chosen enhancement.
            state
                .trait_enhancements
                .entry(trait_id.clone())
                .or_default()
                .push(enhancement_id.clone());
        }
        Event::FocusTokenEarned { .. } => {
            // cumulative_levels_gained is already updated by the LevelUp fold.
            // This event is a wire signal only; no additional state mutation.
        }
        Event::FocusPhaseActivated { pattern, traits } => {
            // Validate: trait count must match pattern's skill count.
            if traits.len() != pattern.skill_count() {
                #[cfg(debug_assertions)]
                eprintln!(
                    "[seed-core] FocusPhaseActivated ignored — traits.len()={} \
                     does not match pattern skill_count={}",
                    traits.len(),
                    pattern.skill_count()
                );
                return;
            }
            // Validate: no duplicate trait IDs (m8). Duplicates collapse the
            // allocation while still costing a token — reject defensively.
            {
                let mut seen = std::collections::BTreeSet::new();
                for t in traits {
                    if !seen.insert(t) {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "[seed-core] FocusPhaseActivated ignored — duplicate trait_id: {}",
                            t.0
                        );
                        return;
                    }
                }
            }
            // Validate: all trait IDs must exist in state (m9). Phantom traits
            // silently waste tokens — reject defensively.
            for t in traits {
                if !state.traits.contains_key(t) {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "[seed-core] FocusPhaseActivated ignored — unknown trait_id: {}",
                        t.0
                    );
                    return;
                }
            }
            // Validate: must have at least one available token.
            if tokens_available(state) == 0 {
                #[cfg(debug_assertions)]
                eprintln!(
                    "[seed-core] FocusPhaseActivated ignored — no tokens available \
                     (cumulative={}, spent={})",
                    state.cumulative_levels_gained, state.tokens_spent
                );
                return;
            }
            // Spend the token.
            state.tokens_spent = state.tokens_spent.saturating_add(1);
            // Build allocations: each trait gets pattern.arrows_per_skill() arrows.
            let arrows = pattern.arrows_per_skill();
            let allocations: Vec<(TraitId, u8)> =
                traits.iter().map(|t| (t.clone(), arrows)).collect();
            // Replace active focus (no stacking — phases replace each other).
            state.active_focus = Some(FocusPhase {
                pattern: pattern.clone(),
                allocations,
            });
        }
        Event::TierChanged { .. } | Event::CompanionAwakened { .. } | Event::Unknown { .. } => {}
        Event::ConfigChanged { key, value } => match key.as_str() {
            "palette" => {
                if let Some(s) = value.as_str() {
                    state.palette = s.to_string();
                }
            }
            "snooze_min" => {
                if let Some(n) = value.as_u64() {
                    state.snooze_min = n as u32;
                }
            }
            "notif_style" => {
                if let Some(s) = value.as_str() {
                    state.notif_style = s.to_string();
                }
            }
            "xp_multiplier" => {
                if let Some(n) = value.as_u64() {
                    state.xp_multiplier = n.clamp(1, 1000) as u32;
                }
            }
            _ => {}
        },
    }
}

// ---------------------------------------------------------------------------
// Focus token helpers
// ---------------------------------------------------------------------------

/// How many focus tokens are currently available to spend.
///
/// Derived from `cumulative_levels_gained / 99 - tokens_spent`.
/// Earned tokens never decrement on integrate — the regrind counts forward.
pub fn tokens_available(state: &State) -> u32 {
    (state.cumulative_levels_gained / 99).saturating_sub(state.tokens_spent)
}

/// Whether the user can activate a focus phase right now.
pub fn can_activate_phase(state: &State) -> bool {
    tokens_available(state) > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::initial_state;
    use pretty_assertions::assert_eq;

    fn fixed_ts() -> DateTime<Utc> {
        "2026-04-22T12:00:00Z".parse().unwrap()
    }

    fn round_trip(event: Event) -> Event {
        let env = to_envelope(&event, fixed_ts());
        let json = serde_json::to_string(&env).unwrap();
        let env2: EventEnvelope = serde_json::from_str(&json).unwrap();
        from_envelope(env2).unwrap()
    }

    // -----------------------------------------------------------------------
    // Round-trip tests (known variants)
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Unknown variant: round-trip preserves kind + data (Fix 1)
    // -----------------------------------------------------------------------

    #[test]
    fn unknown_event_round_trips_without_data_loss() {
        let original = Event::Unknown {
            kind: "seed.future.thing".to_string(),
            data: serde_json::json!({"x": 42, "nested": {"y": true}}),
        };
        let env = to_envelope(&original, fixed_ts());
        // The envelope should preserve the original kind, not rewrite to "seed.unknown".
        assert_eq!(env.kind, "seed.future.thing");
        let json = serde_json::to_string(&env).unwrap();
        let env2: EventEnvelope = serde_json::from_str(&json).unwrap();
        let recovered = from_envelope(env2).unwrap();
        match &recovered {
            Event::Unknown { kind, data } => {
                assert_eq!(kind, "seed.future.thing");
                assert_eq!(*data, serde_json::json!({"x": 42, "nested": {"y": true}}));
            }
            other => panic!("expected Event::Unknown, got {other:?}"),
        }
        // Byte-exact data check.
        assert_eq!(recovered, original);
    }

    #[test]
    fn from_envelope_known_kind_malformed_data_returns_err() {
        // A known kind with an incompatible data payload should return Err,
        // not silently degrade to Unknown.
        let env = EventEnvelope {
            v: 1,
            ts: fixed_ts(),
            kind: "seed.reminder.completed".to_string(),
            data: serde_json::json!({"completely_wrong": "schema"}),
        };
        assert!(
            from_envelope(env).is_err(),
            "known kind with bad data must return Err, not Unknown"
        );
    }

    // -----------------------------------------------------------------------
    // apply_event: ReminderCompleted validation no-ops (Fix 2)
    // -----------------------------------------------------------------------

    const NOW_MS: i64 = 1_745_000_000_000;

    #[test]
    fn apply_reminder_completed_unknown_reminder_id_is_noop() {
        let mut state = initial_state(NOW_MS);
        let before_total = state.completed_total;
        let before_log_len = state.log.len();

        apply_event(
            &mut state,
            &Event::ReminderCompleted {
                reminder_id: ReminderId("nonexistent_reminder".into()),
                xp_gained: 50,
                trait_id: TraitId("flow".into()),
                new_xp: 500,
                streak: 1,
                at_ms: NOW_MS,
            },
        );

        assert_eq!(
            state.completed_total, before_total,
            "completed_total must not increment for unknown reminder_id"
        );
        assert_eq!(
            state.log.len(),
            before_log_len,
            "log must not grow for unknown reminder_id"
        );
    }

    #[test]
    fn apply_reminder_completed_unknown_trait_id_is_noop() {
        let mut state = initial_state(NOW_MS);
        let before_total = state.completed_total;
        let before_log_len = state.log.len();

        apply_event(
            &mut state,
            &Event::ReminderCompleted {
                reminder_id: ReminderId("water".into()),
                xp_gained: 50,
                trait_id: TraitId("nonexistent_trait".into()),
                new_xp: 500,
                streak: 1,
                at_ms: NOW_MS,
            },
        );

        assert_eq!(
            state.completed_total, before_total,
            "completed_total must not increment for unknown trait_id"
        );
        assert_eq!(
            state.log.len(),
            before_log_len,
            "log must not grow for unknown trait_id"
        );
    }

    #[test]
    fn apply_reminder_completed_both_unknown_is_noop() {
        let mut state = initial_state(NOW_MS);
        let before_total = state.completed_total;
        let before_log_len = state.log.len();

        apply_event(
            &mut state,
            &Event::ReminderCompleted {
                reminder_id: ReminderId("bad_reminder".into()),
                xp_gained: 50,
                trait_id: TraitId("bad_trait".into()),
                new_xp: 500,
                streak: 1,
                at_ms: NOW_MS,
            },
        );

        assert_eq!(state.completed_total, before_total);
        assert_eq!(state.log.len(), before_log_len);
    }

    #[test]
    fn apply_reminder_completed_saturating_add_no_overflow() {
        let mut state = initial_state(NOW_MS);
        state.completed_total = u32::MAX;
        if let Some(rt) = state.reminders.get_mut(&ReminderId("water".into())) {
            rt.total_done = u32::MAX;
        }

        // Must not panic; should saturate at u32::MAX.
        apply_event(
            &mut state,
            &Event::ReminderCompleted {
                reminder_id: ReminderId("water".into()),
                xp_gained: 50,
                trait_id: TraitId("flow".into()),
                new_xp: 500,
                streak: 1,
                at_ms: NOW_MS,
            },
        );

        assert_eq!(state.completed_total, u32::MAX);
        assert_eq!(
            state.reminders[&ReminderId("water".into())].total_done,
            u32::MAX
        );
    }

    #[test]
    fn apply_valid_reminder_completed_mutates_state() {
        let mut state = initial_state(NOW_MS);
        let before_total = state.completed_total;
        let completion_ms = NOW_MS + 5_000;

        apply_event(
            &mut state,
            &Event::ReminderCompleted {
                reminder_id: ReminderId("water".into()),
                xp_gained: 74,
                trait_id: TraitId("flow".into()),
                new_xp: 1234,
                streak: 5,
                at_ms: completion_ms,
            },
        );

        assert_eq!(state.completed_total, before_total + 1);
        assert_eq!(*state.traits.get(&TraitId("flow".into())).unwrap(), 1234);
        assert_eq!(state.reminders[&ReminderId("water".into())].streak, 5);
    }

    // -----------------------------------------------------------------------
    // Snapshot tests: byte-equal JSON for all 11 known variants + Unknown (Fix 4)
    // -----------------------------------------------------------------------

    /// Schema lock: byte-equal JSON for ReminderCompleted.
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
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        // at_ms is serialized; fields are alphabetical (serde_json BTreeMap-style).
        let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.reminder.completed","data":{"at_ms":1745000000000,"new_xp":1234,"reminder_id":"water","streak":5,"trait_id":"flow","xp_gained":74}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn snapshot_reminder_snoozed_json_shape() {
        let event = Event::ReminderSnoozed {
            reminder_id: ReminderId("eyes".into()),
            until_ms: 9_999_999,
            snooze_min: 10,
        };
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.reminder.snoozed","data":{"reminder_id":"eyes","snooze_min":10,"until_ms":9999999}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn snapshot_reminder_enabled_json_shape() {
        let event = Event::ReminderEnabled {
            reminder_id: ReminderId("med".into()),
        };
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.reminder.enabled","data":{"reminder_id":"med"}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn snapshot_reminder_disabled_json_shape() {
        let event = Event::ReminderDisabled {
            reminder_id: ReminderId("med".into()),
        };
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.reminder.disabled","data":{"reminder_id":"med"}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn snapshot_reminder_pinned_json_shape() {
        let event = Event::ReminderPinned {
            reminder_id: ReminderId("walk".into()),
        };
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.reminder.pinned","data":{"reminder_id":"walk"}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn snapshot_reminder_unpinned_json_shape() {
        let event = Event::ReminderUnpinned {
            reminder_id: ReminderId("walk".into()),
        };
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.reminder.unpinned","data":{"reminder_id":"walk"}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn snapshot_trait_xp_changed_json_shape() {
        let event = Event::TraitXpChanged {
            trait_id: TraitId("core".into()),
            delta: 55,
            new_xp: 500,
        };
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.trait.xp_changed","data":{"delta":55,"new_xp":500,"trait_id":"core"}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn snapshot_level_up_json_shape() {
        let event = Event::LevelUp {
            trait_id: TraitId("reach".into()),
            old_level: 4,
            new_level: 5,
            new_xp: 1154,
        };
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        // old_level is serialized alphabetically before new_level/new_xp/trait_id.
        let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.trait.level_up","data":{"new_level":5,"new_xp":1154,"old_level":4,"trait_id":"reach"}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn snapshot_tier_changed_json_shape() {
        let event = Event::TierChanged {
            from: Tier::Seed,
            to: Tier::Sprout,
            total_level: 20,
        };
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.companion.tier_changed","data":{"from":"Seed","to":"Sprout","total_level":20}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn snapshot_companion_awakened_json_shape() {
        let event = Event::CompanionAwakened { glyph_seed: 42 };
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.companion.awakened","data":{"glyph_seed":42}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn snapshot_config_changed_json_shape() {
        let event = Event::ConfigChanged {
            key: "palette".to_string(),
            value: serde_json::json!("dusk"),
        };
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.config.changed","data":{"key":"palette","value":"dusk"}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn round_trip_reminder_notified() {
        let e = Event::ReminderNotified {
            reminder_id: ReminderId("water".into()),
            at_ms: 1_745_000_000_000,
        };
        assert_eq!(round_trip(e.clone()), e);
    }

    #[test]
    fn snapshot_reminder_notified_json_shape() {
        let event = Event::ReminderNotified {
            reminder_id: ReminderId("water".into()),
            at_ms: 1_745_000_000_000,
        };
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.reminder.notified","data":{"at_ms":1745000000000,"reminder_id":"water"}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn apply_reminder_notified_updates_last_notified_ms() {
        let mut state = initial_state(NOW_MS);
        apply_event(
            &mut state,
            &Event::ReminderNotified {
                reminder_id: ReminderId("water".into()),
                at_ms: 9_999_999,
            },
        );
        assert_eq!(
            state.reminders[&ReminderId("water".into())].last_notified_ms,
            9_999_999
        );
    }

    #[test]
    fn apply_reminder_notified_unknown_id_is_noop() {
        let mut state = initial_state(NOW_MS);
        apply_event(
            &mut state,
            &Event::ReminderNotified {
                reminder_id: ReminderId("does_not_exist".into()),
                at_ms: 9_999_999,
            },
        );
        // No panic, no state corruption — all reminders still at 0.
        for rt in state.reminders.values() {
            assert_eq!(rt.last_notified_ms, 0);
        }
    }

    // -----------------------------------------------------------------------
    // ReminderIntervalChanged: round-trip + snapshot + apply_event
    // -----------------------------------------------------------------------

    #[test]
    fn round_trip_reminder_interval_changed() {
        let e = Event::ReminderIntervalChanged {
            reminder_id: ReminderId("water".into()),
            minutes: 60,
        };
        assert_eq!(round_trip(e.clone()), e);
    }

    #[test]
    fn snapshot_reminder_interval_changed_json_shape() {
        let event = Event::ReminderIntervalChanged {
            reminder_id: ReminderId("water".into()),
            minutes: 60,
        };
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.reminder.interval_changed","data":{"minutes":60,"reminder_id":"water"}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn apply_reminder_interval_changed_updates_interval_min() {
        let mut state = initial_state(NOW_MS);
        // Verify initial interval from static catalog (water = 45 min).
        assert_eq!(
            state.reminders[&ReminderId("water".into())].interval_min,
            45
        );
        apply_event(
            &mut state,
            &Event::ReminderIntervalChanged {
                reminder_id: ReminderId("water".into()),
                minutes: 90,
            },
        );
        assert_eq!(
            state.reminders[&ReminderId("water".into())].interval_min,
            90
        );
    }

    // -----------------------------------------------------------------------
    // ReminderSkipped: round-trip + snapshot + apply_event
    // -----------------------------------------------------------------------

    #[test]
    fn round_trip_reminder_skipped() {
        let e = Event::ReminderSkipped {
            at_ms: 1_745_000_000_000,
            missed_cycles: 3,
            reminder_id: ReminderId("water".into()),
            was_snoozed: false,
        };
        assert_eq!(round_trip(e.clone()), e);
    }

    #[test]
    fn round_trip_reminder_skipped_with_was_snoozed() {
        let e = Event::ReminderSkipped {
            at_ms: 1_745_000_000_000,
            missed_cycles: 1,
            reminder_id: ReminderId("water".into()),
            was_snoozed: true,
        };
        assert_eq!(round_trip(e.clone()), e);
    }

    #[test]
    fn snapshot_reminder_skipped_json_shape() {
        let event = Event::ReminderSkipped {
            at_ms: 1_745_000_000_000,
            missed_cycles: 3,
            reminder_id: ReminderId("water".into()),
            was_snoozed: false,
        };
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        // Fields serialized in declaration order: at_ms, missed_cycles, reminder_id, was_snoozed.
        let expected = r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.reminder.skipped","data":{"at_ms":1745000000000,"missed_cycles":3,"reminder_id":"water","was_snoozed":false}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn apply_reminder_skipped_resets_last_done_ms_and_streak() {
        let mut state = initial_state(NOW_MS);
        let t_old = NOW_MS - 100 * 60 * 1000;
        {
            let rt = state
                .reminders
                .get_mut(&ReminderId("water".into()))
                .unwrap();
            rt.last_done_ms = t_old;
            rt.streak = 5;
        }

        // was_snoozed: false → streak resets, total_missed increments.
        apply_event(
            &mut state,
            &Event::ReminderSkipped {
                at_ms: NOW_MS,
                missed_cycles: 1,
                reminder_id: ReminderId("water".into()),
                was_snoozed: false,
            },
        );

        let rt = &state.reminders[&ReminderId("water".into())];
        assert_eq!(rt.last_done_ms, NOW_MS, "last_done_ms must be set to at_ms");
        assert_eq!(rt.streak, 0, "streak must be reset to 0");
        assert_eq!(rt.total_missed, 1, "total_missed must increment");
    }

    #[test]
    fn apply_reminder_skipped_increments_total_missed() {
        let mut state = initial_state(NOW_MS);
        {
            let rt = state
                .reminders
                .get_mut(&ReminderId("water".into()))
                .unwrap();
            rt.total_missed = 0;
        }

        apply_event(
            &mut state,
            &Event::ReminderSkipped {
                at_ms: NOW_MS,
                missed_cycles: 1,
                reminder_id: ReminderId("water".into()),
                was_snoozed: false,
            },
        );
        assert_eq!(
            state.reminders[&ReminderId("water".into())].total_missed,
            1,
            "total_missed must be 1 after first skip"
        );

        apply_event(
            &mut state,
            &Event::ReminderSkipped {
                at_ms: NOW_MS + 1,
                missed_cycles: 1,
                reminder_id: ReminderId("water".into()),
                was_snoozed: false,
            },
        );
        assert_eq!(
            state.reminders[&ReminderId("water".into())].total_missed,
            2,
            "total_missed must be 2 after second skip"
        );
    }

    #[test]
    fn apply_reminder_skipped_was_snoozed_preserves_streak_and_missed() {
        let mut state = initial_state(NOW_MS);
        let t_old = NOW_MS - 100 * 60 * 1000;
        {
            let rt = state
                .reminders
                .get_mut(&ReminderId("water".into()))
                .unwrap();
            rt.last_done_ms = t_old;
            rt.streak = 5;
            rt.total_missed = 2;
            // Snooze was active during this cycle.
            rt.snoozed_until_ms = t_old + 10 * 60 * 1000;
        }

        apply_event(
            &mut state,
            &Event::ReminderSkipped {
                at_ms: NOW_MS,
                missed_cycles: 1,
                reminder_id: ReminderId("water".into()),
                was_snoozed: true,
            },
        );

        let rt = &state.reminders[&ReminderId("water".into())];
        assert_eq!(rt.last_done_ms, NOW_MS, "last_done_ms must still advance");
        assert_eq!(rt.streak, 5, "streak must be preserved when was_snoozed");
        assert_eq!(
            rt.total_missed, 2,
            "total_missed must not increment when was_snoozed"
        );
        assert!(
            state.traits_skipped.is_empty(),
            "traits_skipped must not be populated when was_snoozed"
        );
    }

    #[test]
    fn apply_reminder_skipped_unknown_id_is_noop() {
        let mut state = initial_state(NOW_MS);
        let before: Vec<_> = state
            .reminders
            .values()
            .map(|rt| (rt.total_missed, rt.streak, rt.last_done_ms))
            .collect();

        apply_event(
            &mut state,
            &Event::ReminderSkipped {
                at_ms: NOW_MS,
                missed_cycles: 1,
                reminder_id: ReminderId("does_not_exist".into()),
                was_snoozed: false,
            },
        );

        let after: Vec<_> = state
            .reminders
            .values()
            .map(|rt| (rt.total_missed, rt.streak, rt.last_done_ms))
            .collect();
        assert_eq!(
            before, after,
            "unknown reminder_id must leave all state unchanged"
        );
    }

    // -----------------------------------------------------------------------
    // Per-trait skip aggregation
    // -----------------------------------------------------------------------

    #[test]
    fn apply_reminder_skipped_aggregates_per_trait() {
        let mut state = initial_state(NOW_MS);
        apply_event(
            &mut state,
            &Event::ReminderSkipped {
                at_ms: NOW_MS,
                missed_cycles: 1,
                reminder_id: ReminderId("water".into()),
                was_snoozed: false,
            },
        );
        // water → hydration → flow
        let flow = crate::domain::TraitId("flow".into());
        let stats = state.traits_skipped.get(&flow).expect("flow stats missing");
        assert_eq!(stats.lifetime, 1);
        assert_eq!(stats.recent.len(), 1);
        assert_eq!(stats.recent[0], NOW_MS);
    }

    #[test]
    fn apply_reminder_skipped_was_snoozed_does_not_aggregate() {
        let mut state = initial_state(NOW_MS);
        apply_event(
            &mut state,
            &Event::ReminderSkipped {
                at_ms: NOW_MS,
                missed_cycles: 1,
                reminder_id: ReminderId("water".into()),
                was_snoozed: true,
            },
        );
        assert!(
            state.traits_skipped.is_empty(),
            "traits_skipped must remain empty when was_snoozed"
        );
    }

    #[test]
    fn apply_reminder_skipped_prunes_recent_older_than_7d() {
        let mut state = initial_state(NOW_MS);
        // Pre-seed: one old (8d), one recent (1d).
        let old_ts = NOW_MS - 8 * 86_400_000;
        let recent_ts = NOW_MS - 86_400_000;
        let flow = crate::domain::TraitId("flow".into());
        state
            .traits_skipped
            .entry(flow.clone())
            .or_default()
            .recent
            .push(old_ts);
        state
            .traits_skipped
            .entry(flow.clone())
            .or_default()
            .recent
            .push(recent_ts);

        apply_event(
            &mut state,
            &Event::ReminderSkipped {
                at_ms: NOW_MS,
                missed_cycles: 1,
                reminder_id: ReminderId("water".into()),
                was_snoozed: false,
            },
        );

        let stats = state.traits_skipped.get(&flow).unwrap();
        // old_ts (8d) pruned; recent_ts (1d) + NOW_MS survive = 2.
        assert_eq!(stats.recent.len(), 2, "8d-old entry must be pruned");
        assert!(
            !stats.recent.contains(&old_ts),
            "8d-old entry must not be present"
        );
    }

    #[test]
    fn traits_skipped_round_trip_serde() {
        use crate::state::TraitSkipStats;
        let mut state = initial_state(NOW_MS);
        let flow = crate::domain::TraitId("flow".into());
        state.traits_skipped.insert(
            flow,
            TraitSkipStats {
                lifetime: 7,
                recent: vec![NOW_MS - 1000, NOW_MS],
            },
        );
        let json = serde_json::to_string(&state).unwrap();
        let state2: crate::state::State = serde_json::from_str(&json).unwrap();
        let stats = state2
            .traits_skipped
            .get(&crate::domain::TraitId("flow".into()))
            .unwrap();
        assert_eq!(stats.lifetime, 7);
        assert_eq!(stats.recent.len(), 2);
    }

    #[test]
    fn snapshot_unknown_event_json_shape() {
        // Unknown events must pass through to_envelope verbatim — kind and data preserved.
        let event = Event::Unknown {
            kind: "seed.future.thing".to_string(),
            data: serde_json::json!({"x": 42}),
        };
        let ts: DateTime<Utc> = "2026-04-22T12:00:00Z".parse().unwrap();
        let env = to_envelope(&event, ts);
        let json = serde_json::to_string(&env).unwrap();
        let expected =
            r#"{"v":1,"ts":"2026-04-22T12:00:00Z","kind":"seed.future.thing","data":{"x":42}}"#;
        assert_eq!(json, expected);
    }

    // -----------------------------------------------------------------------
    // Fix 1 (Wave 3.1): ReminderCompleted stamps last_done_ms
    // -----------------------------------------------------------------------

    /// Complete at T → state's last_done_ms == T.
    #[test]
    fn apply_reminder_completed_stamps_last_done_ms() {
        let mut state = initial_state(NOW_MS);
        let completion_ms = NOW_MS + 120_000; // 2 minutes later

        apply_event(
            &mut state,
            &Event::ReminderCompleted {
                reminder_id: ReminderId("water".into()),
                xp_gained: 74,
                trait_id: TraitId("flow".into()),
                new_xp: 1234,
                streak: 1,
                at_ms: completion_ms,
            },
        );

        assert_eq!(
            state.reminders[&ReminderId("water".into())].last_done_ms,
            completion_ms,
            "last_done_ms must equal the at_ms from the completion event"
        );
    }

    /// Replay from scratch: envelope timestamps are respected across two completions.
    #[test]
    fn replay_completion_timestamps_respected() {
        let mut state = initial_state(NOW_MS);
        let t1 = NOW_MS + 60_000;
        let t2 = NOW_MS + 300_000;

        apply_event(
            &mut state,
            &Event::ReminderCompleted {
                reminder_id: ReminderId("water".into()),
                xp_gained: 74,
                trait_id: TraitId("flow".into()),
                new_xp: 1000,
                streak: 1,
                at_ms: t1,
            },
        );
        assert_eq!(
            state.reminders[&ReminderId("water".into())].last_done_ms,
            t1
        );

        apply_event(
            &mut state,
            &Event::ReminderCompleted {
                reminder_id: ReminderId("water".into()),
                xp_gained: 74,
                trait_id: TraitId("flow".into()),
                new_xp: 2000,
                streak: 2,
                at_ms: t2,
            },
        );
        assert_eq!(
            state.reminders[&ReminderId("water".into())].last_done_ms,
            t2,
            "second completion must advance last_done_ms to t2"
        );
    }

    #[test]
    fn apply_config_changed_xp_multiplier_updates_state() {
        let mut state = initial_state(NOW_MS);
        assert_eq!(state.xp_multiplier, 1);

        apply_event(
            &mut state,
            &Event::ConfigChanged {
                key: "xp_multiplier".into(),
                value: serde_json::json!(10u32),
            },
        );
        assert_eq!(state.xp_multiplier, 10);

        // Clamp: value > 1000 becomes 1000.
        apply_event(
            &mut state,
            &Event::ConfigChanged {
                key: "xp_multiplier".into(),
                value: serde_json::json!(9999u32),
            },
        );
        assert_eq!(state.xp_multiplier, 1000);

        // Clamp: value 0 becomes 1.
        apply_event(
            &mut state,
            &Event::ConfigChanged {
                key: "xp_multiplier".into(),
                value: serde_json::json!(0u32),
            },
        );
        assert_eq!(state.xp_multiplier, 1);
    }
}
