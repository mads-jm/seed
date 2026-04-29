/// Scheduler: scans reminders each tick, fires notifications on Dormant→Due
/// transitions, applies XP drain for overdue reminders.
///
/// All side effects go through the `Notifier` trait so tests use a mock.
use chrono::{DateTime, Timelike, Utc};
use seed_core::{
    Config, Event, State, TraitId,
    domain::{REMINDERS, ReminderState, reminder_status_with_interval},
    levels::{level_for_xp, xp_drain},
};

use crate::notify::Notifier;

/// `xp_drain()` returns a value scaled ×100 (i.e., 35 = 0.35 XP/tick).
/// We accumulate and apply a minimum of 1 XP per tick when any drain is due,
/// to ensure the mechanic is visible. Exact fractional drain can be implemented
/// in a later wave with a persistent accumulator in State.
const XP_DRAIN_SCALE: u64 = 100;

/// Run one scheduler tick. Returns all events to commit.
///
/// `now` is injected so tests can use a fixed clock.
pub async fn tick(
    state: &mut State,
    config: &Config,
    notifier: &dyn Notifier,
    now: DateTime<Utc>,
) -> Vec<Event> {
    let now_ms = now.timestamp_millis();
    let now_hour = now.hour() as u8;
    let (active_start, active_end) = config.active_hours;
    let in_active_hours = now_hour >= active_start && now_hour < active_end;

    let mut events: Vec<Event> = Vec::new();

    for reminder in REMINDERS {
        let rid = reminder.reminder_id();
        let rt = match state.reminders.get(&rid) {
            Some(r) => r,
            None => continue,
        };

        if !rt.enabled {
            continue;
        }

        // Use runtime interval so user overrides (ReminderIntervalChanged) are respected.
        let status =
            reminder_status_with_interval(rt.interval_min, rt.last_done_ms, rt.enabled, now_ms);

        match status.state {
            ReminderState::Due => {
                // Notification debounce: only notify if we haven't already notified
                // since this reminder last became due.
                let last_done_ms = rt.last_done_ms;
                let last_notified_ms = rt.last_notified_ms;
                let already_notified = last_notified_ms > last_done_ms;

                if !already_notified && in_active_hours {
                    // Check snooze: snoozed_until_ms > now_ms means snoozed.
                    let snoozed = rt.snoozed_until_ms > now_ms;
                    if !snoozed {
                        let title = format!("seed · {}", reminder.name);
                        let body = reminder.desc;
                        if let Err(e) = notifier.notify(&rid, &title, body).await {
                            tracing::warn!(reminder = %rid.0, "notification failed: {e}");
                        }
                        events.push(Event::ReminderNotified {
                            reminder_id: rid.clone(),
                            at_ms: now_ms,
                        });
                    }
                }
            }

            ReminderState::Overdue => {
                let interval_ms = rt.interval_min as i64 * 60 * 1000;
                let overdue_ms = -status.ms_left;

                // Auto-skip rollover: if overdue by more than one full interval
                // (i.e. now > 2× interval since last_done_ms), emit a skip event
                // and short-circuit. No XP drain this tick — skip and drain are
                // mutually exclusive per tick. Skip is NOT gated by active hours
                // so state cleanup runs during sleep.
                if overdue_ms > interval_ms {
                    let missed = (overdue_ms / interval_ms) as u32;
                    // Snoozed if the user explicitly deferred at any point in
                    // this overdue cycle — i.e. snoozed_until_ms is later than
                    // last_done_ms (not necessarily still active right now).
                    let was_snoozed = rt.snoozed_until_ms > rt.last_done_ms;
                    events.push(Event::ReminderSkipped {
                        at_ms: now_ms,
                        missed_cycles: missed,
                        reminder_id: rid.clone(),
                        was_snoozed,
                    });
                    continue;
                }

                // XP drain is gated by active hours: overnight idleness must not
                // bleed XP indefinitely. Drain only runs inside the active window.
                if !in_active_hours {
                    continue;
                }

                // --- XP drain ---
                // Find the trait bound to this reminder's category.
                let cat_id = reminder.cat;
                let trait_id = match trait_id_for_category(cat_id) {
                    Some(t) => t,
                    None => continue,
                };

                let current_xp = match state.traits.get(&trait_id) {
                    Some(&xp) => xp,
                    None => continue,
                };

                let level = level_for_xp(current_xp);
                // xp_drain returns value ×100; divide to get actual XP to drain.
                let _ = level; // xp_drain is level-independent in v0
                let drain_raw = xp_drain() as u64;
                // drain_raw is ×100 (35 = 0.35). Floor to at least 1 so the
                // mechanic is always observable; exact fractional accumulation
                // is deferred to Wave 4.
                let drain = (drain_raw / XP_DRAIN_SCALE).max(1);

                if current_xp > 0 {
                    let new_xp = current_xp.saturating_sub(drain);
                    events.push(Event::TraitXpChanged {
                        trait_id,
                        delta: -(drain as i64),
                        new_xp,
                    });
                }
            }

            ReminderState::Dormant | ReminderState::Off => {}
        }
    }

    events
}

/// Look up the trait id bound to a category id from the static catalog.
fn trait_id_for_category(cat_id: &str) -> Option<TraitId> {
    seed_core::domain::CATEGORIES
        .iter()
        .find(|c| c.id == cat_id)
        .map(|c| TraitId(c.trait_id.to_owned()))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::MockNotifier;
    use seed_core::{Config, ReminderId, TraitId, apply_event, initial_state};
    use std::sync::Arc;

    fn make_now(ms: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(ms).unwrap()
    }

    fn active_time() -> DateTime<Utc> {
        // 2026-04-22 14:00 UTC — within default active hours (7-22).
        "2026-04-22T14:00:00Z".parse().unwrap()
    }

    fn active_time_ms() -> i64 {
        active_time().timestamp_millis()
    }

    fn inactive_time() -> DateTime<Utc> {
        // 2026-04-22 03:00 UTC — outside default active hours (7-22).
        "2026-04-22T03:00:00Z".parse().unwrap()
    }

    /// Build a state where "water" (45-min interval) is Due at `active_time()`.
    fn state_with_water_due() -> State {
        let now_ms = active_time_ms();
        let mut s = initial_state(now_ms);
        // Set last_done 46 min ago relative to active_time → Due (>45 min, <67.5 min).
        let due_last_done = now_ms - 46 * 60 * 1000;
        s.reminders
            .get_mut(&ReminderId("water".into()))
            .unwrap()
            .last_done_ms = due_last_done;
        s
    }

    #[tokio::test]
    async fn notifies_due_reminder_in_active_hours() {
        let mut state = state_with_water_due();
        let config = Config::default();
        let notifier = Arc::new(MockNotifier::new());

        let events = tick(&mut state, &config, notifier.as_ref(), active_time()).await;

        assert_eq!(
            notifier.call_count(),
            1,
            "expected exactly one notification"
        );
        assert!(notifier.was_called_for("water"));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::ReminderNotified { .. })),
            "expected ReminderNotified event"
        );
    }

    #[tokio::test]
    async fn no_notification_outside_active_hours() {
        let mut state = state_with_water_due();
        let config = Config::default();
        let notifier = MockNotifier::new();

        let events = tick(&mut state, &config, &notifier, inactive_time()).await;

        assert_eq!(
            notifier.call_count(),
            0,
            "must not notify outside active hours"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::ReminderNotified { .. })),
            "no ReminderNotified event expected"
        );
    }

    #[tokio::test]
    async fn no_double_notify_same_due_window() {
        let mut state = state_with_water_due();
        let config = Config::default();
        let notifier = MockNotifier::new();

        let now = active_time();
        let now_ms = now.timestamp_millis();

        // First tick: fires notification, emits ReminderNotified.
        let events1 = tick(&mut state, &config, &notifier, now).await;
        assert_eq!(notifier.call_count(), 1);

        // Apply the ReminderNotified event to state (simulates commit).
        for e in &events1 {
            apply_event(&mut state, e);
        }

        // Second tick at the same time: must NOT re-notify.
        let events2 = tick(&mut state, &config, &notifier, make_now(now_ms + 30_000)).await;
        assert_eq!(notifier.call_count(), 1, "must not double-notify");
        assert!(
            !events2
                .iter()
                .any(|e| matches!(e, Event::ReminderNotified { .. })),
            "no second ReminderNotified"
        );
    }

    #[tokio::test]
    async fn snoozed_reminder_not_notified() {
        let mut state = state_with_water_due();
        let config = Config::default();
        let notifier = MockNotifier::new();

        // Snooze water for 10 minutes from now.
        let now = active_time();
        let now_ms = now.timestamp_millis();
        state
            .reminders
            .get_mut(&ReminderId("water".into()))
            .unwrap()
            .snoozed_until_ms = now_ms + 10 * 60 * 1000;

        let events = tick(&mut state, &config, &notifier, now).await;
        assert_eq!(notifier.call_count(), 0, "snoozed reminders must not fire");
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::ReminderNotified { .. }))
        );
    }

    /// Water last_done = now − 80 min.
    /// overdue_ms = 80*60*1000 − 45*60*1000 = 35*60*1000 (< interval_ms = 45*60*1000).
    /// Must drain XP (active hours), must NOT emit ReminderSkipped.
    #[tokio::test]
    async fn overdue_under_2x_drains_xp() {
        let now_ms = active_time_ms();
        let mut state = initial_state(now_ms);
        // 80 min ago → elapsed = 80 min, overdue_ms = 35 min < interval_ms = 45 min.
        state
            .reminders
            .get_mut(&ReminderId("water".into()))
            .unwrap()
            .last_done_ms = now_ms - 80 * 60 * 1000;

        *state.traits.get_mut(&TraitId("flow".into())).unwrap() = 1000;

        let config = Config::default();
        let notifier = MockNotifier::new();
        let events = tick(&mut state, &config, &notifier, active_time()).await;

        let drain_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::TraitXpChanged { delta, .. } if *delta < 0))
            .collect();
        assert!(
            !drain_events.is_empty(),
            "expected at least one XP drain event when overdue_ms < interval_ms"
        );

        let skip_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::ReminderSkipped { .. }))
            .collect();
        assert!(
            skip_events.is_empty(),
            "must not emit ReminderSkipped when overdue_ms < interval_ms"
        );
    }

    /// Water last_done = now − 100 min.
    /// overdue_ms = 55 min > interval_ms = 45 min → skip fires, drain does NOT.
    #[tokio::test]
    async fn overdue_over_2x_emits_skip_not_drain() {
        let now_ms = active_time_ms();
        let mut state = initial_state(now_ms);
        // 100 min ago → elapsed = 100 min, overdue_ms = 55 min > interval_ms = 45 min.
        state
            .reminders
            .get_mut(&ReminderId("water".into()))
            .unwrap()
            .last_done_ms = now_ms - 100 * 60 * 1000;

        *state.traits.get_mut(&TraitId("flow".into())).unwrap() = 1000;

        let config = Config::default();
        let notifier = MockNotifier::new();
        let events = tick(&mut state, &config, &notifier, active_time()).await;

        let water_skip: Vec<_> = events
            .iter()
            .filter(|e| {
                matches!(e, Event::ReminderSkipped { reminder_id, .. }
                    if reminder_id.0 == "water")
            })
            .collect();
        assert_eq!(
            water_skip.len(),
            1,
            "expected exactly one ReminderSkipped for water"
        );
        // No snooze was set → was_snoozed must be false.
        if let Some(Event::ReminderSkipped { was_snoozed, .. }) = water_skip.first().copied() {
            assert!(
                !was_snoozed,
                "was_snoozed must be false when no snooze was set"
            );
        }

        let flow_drain: Vec<_> = events
            .iter()
            .filter(|e| {
                matches!(e, Event::TraitXpChanged { trait_id, delta, .. }
                    if trait_id.0 == "flow" && *delta < 0)
            })
            .collect();
        assert!(
            flow_drain.is_empty(),
            "must not emit XP drain when ReminderSkipped fires"
        );
    }

    /// Snooze was active during the overdue cycle (snoozed_until_ms > last_done_ms,
    /// but snooze has already expired). was_snoozed must be true.
    #[tokio::test]
    async fn auto_skip_emits_was_snoozed_true_when_snooze_active_in_cycle() {
        let now_ms = active_time_ms();
        let mut state = initial_state(now_ms);
        let last_done = now_ms - 100 * 60 * 1000; // 100 min ago
        {
            let rt = state
                .reminders
                .get_mut(&ReminderId("water".into()))
                .unwrap();
            rt.last_done_ms = last_done;
            // Snoozed during the cycle but snooze has since expired.
            rt.snoozed_until_ms = now_ms - 30 * 60 * 1000;
        }

        let config = Config::default();
        let notifier = MockNotifier::new();
        let events = tick(&mut state, &config, &notifier, active_time()).await;

        let skip = events.iter().find(|e| {
            matches!(e, Event::ReminderSkipped { reminder_id, .. }
                if reminder_id.0 == "water")
        });
        assert!(skip.is_some(), "expected ReminderSkipped for water");
        if let Some(Event::ReminderSkipped { was_snoozed, .. }) = skip {
            assert!(
                *was_snoozed,
                "was_snoozed must be true when snooze_until > last_done"
            );
        }
    }

    /// No snooze was ever set (snoozed_until_ms = 0). was_snoozed must be false.
    #[tokio::test]
    async fn auto_skip_emits_was_snoozed_false_when_no_snooze_in_cycle() {
        let now_ms = active_time_ms();
        let mut state = initial_state(now_ms);
        {
            let rt = state
                .reminders
                .get_mut(&ReminderId("water".into()))
                .unwrap();
            rt.last_done_ms = now_ms - 100 * 60 * 1000;
            rt.snoozed_until_ms = 0; // default — no snooze ever set
        }

        let config = Config::default();
        let notifier = MockNotifier::new();
        let events = tick(&mut state, &config, &notifier, active_time()).await;

        let skip = events.iter().find(|e| {
            matches!(e, Event::ReminderSkipped { reminder_id, .. }
                if reminder_id.0 == "water")
        });
        assert!(skip.is_some(), "expected ReminderSkipped for water");
        if let Some(Event::ReminderSkipped { was_snoozed, .. }) = skip {
            assert!(
                !was_snoozed,
                "was_snoozed must be false when snoozed_until_ms = 0"
            );
        }
    }

    /// Water last_done = now − 10 × 45 min = 450 min ago.
    /// elapsed = 450 min, overdue_ms = 450*60*1000 − 45*60*1000 = 405*60*1000 = 9 × interval_ms.
    /// missed_cycles = overdue_ms / interval_ms = 9.
    #[tokio::test]
    async fn auto_skip_reports_correct_missed_cycles() {
        let now_ms = active_time_ms();
        let mut state = initial_state(now_ms);
        let interval_min = 45i64;
        state
            .reminders
            .get_mut(&ReminderId("water".into()))
            .unwrap()
            .last_done_ms = now_ms - 10 * interval_min * 60 * 1000;

        let config = Config::default();
        let notifier = MockNotifier::new();
        let events = tick(&mut state, &config, &notifier, active_time()).await;

        let skip = events.iter().find(|e| {
            matches!(e, Event::ReminderSkipped { reminder_id, .. }
                if reminder_id.0 == "water")
        });
        assert!(skip.is_some(), "expected ReminderSkipped for water");
        if let Some(Event::ReminderSkipped { missed_cycles, .. }) = skip {
            assert_eq!(
                *missed_cycles, 9,
                "missed_cycles must be 9 at 10× interval elapsed"
            );
        }
    }

    /// Skip emission is NOT gated by active hours — state cleanup must run during sleep.
    #[tokio::test]
    async fn auto_skip_during_inactive_hours_still_fires() {
        let now_ms = inactive_time().timestamp_millis();
        let mut state = initial_state(now_ms);
        // Use inactive_time as the reference; 100 min ago → overdue_ms = 55 min > 45 min.
        state
            .reminders
            .get_mut(&ReminderId("water".into()))
            .unwrap()
            .last_done_ms = now_ms - 100 * 60 * 1000;

        let config = Config::default();
        let notifier = MockNotifier::new();
        let events = tick(&mut state, &config, &notifier, inactive_time()).await;

        let water_skip: Vec<_> = events
            .iter()
            .filter(|e| {
                matches!(e, Event::ReminderSkipped { reminder_id, .. }
                    if reminder_id.0 == "water")
            })
            .collect();
        assert_eq!(
            water_skip.len(),
            1,
            "ReminderSkipped must fire during inactive hours"
        );
    }

    /// XP drain is gated by active hours: running at inactive_time must not emit drain.
    #[tokio::test]
    async fn overdue_drain_gated_by_active_hours() {
        let now_ms = inactive_time().timestamp_millis();
        let mut state = initial_state(now_ms);
        // 80 min ago → overdue_ms = 35 min < interval_ms = 45 min → drain path, not skip.
        state
            .reminders
            .get_mut(&ReminderId("water".into()))
            .unwrap()
            .last_done_ms = now_ms - 80 * 60 * 1000;

        *state.traits.get_mut(&TraitId("flow".into())).unwrap() = 1000;

        let config = Config::default();
        let notifier = MockNotifier::new();
        let events = tick(&mut state, &config, &notifier, inactive_time()).await;

        let drain_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::TraitXpChanged { delta, .. } if *delta < 0))
            .collect();
        assert!(
            drain_events.is_empty(),
            "XP drain must not fire outside active hours"
        );
    }

    /// After applying a ReminderSkipped event, the reminder status must return Dormant
    /// with ms_left == interval_ms.
    #[tokio::test]
    async fn auto_skip_then_status_returns_dormant() {
        use seed_core::domain::{ReminderState, reminder_status_with_interval};

        let now_ms = active_time_ms();
        let mut state = initial_state(now_ms);
        state
            .reminders
            .get_mut(&ReminderId("water".into()))
            .unwrap()
            .last_done_ms = now_ms - 100 * 60 * 1000;

        let config = Config::default();
        let notifier = MockNotifier::new();
        let events = tick(&mut state, &config, &notifier, active_time()).await;

        // Apply all emitted events.
        for e in &events {
            apply_event(&mut state, e);
        }

        let rt = &state.reminders[&ReminderId("water".into())];
        let interval_ms = rt.interval_min as i64 * 60 * 1000;
        let status = reminder_status_with_interval(rt.interval_min, rt.last_done_ms, true, now_ms);

        assert_eq!(
            status.state,
            ReminderState::Dormant,
            "reminder must be Dormant after skip is applied"
        );
        assert_eq!(
            status.ms_left, interval_ms,
            "ms_left must equal a full interval after skip"
        );
    }

    #[tokio::test]
    async fn disabled_reminder_not_notified() {
        let mut state = state_with_water_due();
        state
            .reminders
            .get_mut(&ReminderId("water".into()))
            .unwrap()
            .enabled = false;

        let config = Config::default();
        let notifier = MockNotifier::new();

        tick(&mut state, &config, &notifier, active_time()).await;
        assert_eq!(notifier.call_count(), 0);
    }

    #[tokio::test]
    async fn active_hours_boundary_exclusive() {
        // Notification window is [7, 22). Hour 22 itself must NOT trigger.
        let mut state = state_with_water_due();
        let config = Config::default(); // active_hours = (7, 22)
        let notifier = MockNotifier::new();

        // 2026-04-22 22:00 UTC — exactly at the boundary (exclusive end).
        let boundary: DateTime<Utc> = "2026-04-22T22:00:00Z".parse().unwrap();
        let events = tick(&mut state, &config, &notifier, boundary).await;

        assert_eq!(
            notifier.call_count(),
            0,
            "hour 22 is outside active window [7, 22)"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::ReminderNotified { .. }))
        );
    }
}
