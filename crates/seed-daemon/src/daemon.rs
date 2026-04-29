/// Daemon: owns State, drives the event log, IPC server, and scheduler.
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use seed_core::{
    Config, Event, EventEnvelope, ReminderId, State, TraitId, apply_event, events_path,
    initial_state, load as load_config, snapshot_path, to_envelope,
};
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};
use tracing::{error, info, warn};

use crate::{
    event_log::EventLog,
    ipc::{Command, ConnId, run_listener},
    notify::{DesktopNotifier, Notifier},
    schedule::tick as schedule_tick,
    wire::{Action, Message, ResponseResult},
};

// ---------------------------------------------------------------------------
// Snapshot interval constants
// ---------------------------------------------------------------------------
const SNAPSHOT_EVENT_THRESHOLD: usize = 100;
const SNAPSHOT_TIME_SECS: u64 = 300; // 5 minutes
const TICK_INTERVAL_SECS: u64 = 30;

// ---------------------------------------------------------------------------
// Daemon
// ---------------------------------------------------------------------------

pub struct Daemon {
    seed_home: PathBuf,
    state: Arc<RwLock<State>>,
    event_log: Arc<Mutex<EventLog>>,
    config: Config,
    notifier: Arc<dyn Notifier>,
    broadcast_tx: broadcast::Sender<Vec<EventEnvelope>>,
    cmd_rx: mpsc::Receiver<Command>,
    /// Events committed since last snapshot.
    events_since_snapshot: usize,
    /// Per-connection response senders. Keyed by ConnId from ipc.rs.
    /// Responses are routed to the originating connection only (Wave 3.1 Fix 2).
    conn_resp: HashMap<ConnId, mpsc::Sender<Message>>,
}

impl Daemon {
    /// Entry point used by `main`. Accepts an optional oneshot shutdown signal
    /// so the Ctrl-C handler can trigger graceful shutdown (final snapshot +
    /// log flush) without calling `process::exit()` (Wave 3.1 Fix 3).
    pub async fn run_with_shutdown(
        seed_home: PathBuf,
        foreground: bool,
        shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<()> {
        Self::build_and_run(seed_home, foreground, Some(shutdown_rx)).await
    }

    async fn build_and_run(
        seed_home: PathBuf,
        _foreground: bool,
        shutdown_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    ) -> Result<()> {
        let config = load_config(&seed_home).unwrap_or_default();
        let (broadcast_tx, _) = broadcast::channel(256);
        let (cmd_tx, cmd_rx) = mpsc::channel(256);

        let log_path = events_path(&seed_home);
        let snap_path = snapshot_path(&seed_home);

        // Ensure home dir exists.
        std::fs::create_dir_all(&seed_home)
            .with_context(|| format!("failed to create seed home: {}", seed_home.display()))?;

        // Load snapshot then tail events.
        let (mut state, skip_count) = match EventLog::snapshot_read(&snap_path)? {
            Some((s, count)) => {
                info!(events_consumed = count, "loaded snapshot");
                (s, count)
            }
            None => {
                info!("no snapshot — starting fresh");
                (initial_state(chrono::Utc::now().timestamp_millis()), 0)
            }
        };

        // Tail events.jsonl from snapshot offset.
        let tail = EventLog::load_from(&log_path, skip_count)?;
        let tail_count = tail.len();
        for ev in tail {
            apply_event(&mut state, &ev);
        }
        info!(tail_count, "replayed events from log tail");

        // First-ever startup: emit CompanionAwakened.
        let event_log = if !log_path.exists() || (skip_count == 0 && tail_count == 0) {
            let mut log = EventLog::open(&log_path)?;
            // Only emit if this is truly first startup (no events at all).
            if log.event_count == 0 {
                let ev = Event::CompanionAwakened {
                    glyph_seed: state.glyph_seed,
                };
                apply_event(&mut state, &ev);
                log.append(&ev)?;
                info!("companion awakened — first startup");
            }
            log
        } else {
            EventLog::open(&log_path)?
        };

        let mut daemon = Daemon {
            seed_home,
            state: Arc::new(RwLock::new(state)),
            event_log: Arc::new(Mutex::new(event_log)),
            config,
            notifier: Arc::new(DesktopNotifier),
            broadcast_tx,
            cmd_rx,
            events_since_snapshot: 0,
            conn_resp: HashMap::new(),
        };

        daemon.run_inner(cmd_tx, shutdown_rx).await
    }

    async fn run_inner(
        &mut self,
        cmd_tx: mpsc::Sender<Command>,
        shutdown_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    ) -> Result<()> {
        // Start IPC listener — supervised: if it panics, log and trigger shutdown.
        let diff_tx = self.broadcast_tx.clone();
        let ipc_cmd_tx = cmd_tx.clone();
        let ipc_handle = tokio::spawn(async move {
            if let Err(e) = run_listener(ipc_cmd_tx, diff_tx).await {
                warn!("IPC listener exited: {e}");
            }
        });

        // Tick timer — supervised.
        let tick_cmd_tx = cmd_tx.clone();
        let tick_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(TICK_INTERVAL_SECS));
            loop {
                interval.tick().await;
                // Internal tick: throwaway response channel (request_id=0 skips response).
                let (tx, _rx) = mpsc::channel(1);
                let _ = tick_cmd_tx
                    .send(Command::Action {
                        conn_id: 0,
                        request_id: 0,
                        action: Action::TriggerReminderNow { reminder_id: None },
                        resp_tx: tx,
                    })
                    .await;
            }
        });

        // Snapshot timer — supervised.
        let seed_home2 = self.seed_home.clone();
        let state2 = self.state.clone();
        let log2 = self.event_log.clone();
        let snap_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(SNAPSHOT_TIME_SECS));
            interval.tick().await; // skip immediate first tick
            loop {
                interval.tick().await;
                let snap_path = snapshot_path(&seed_home2);
                let state = state2.read().await.clone();
                let count = log2.lock().await.event_count;
                if let Err(e) = EventLog::snapshot_write(&snap_path, &state, count) {
                    warn!("periodic snapshot failed: {e}");
                }
            }
        });

        // Wrap the optional oneshot into a future that can be selected on.
        // If no shutdown_rx was provided (test/direct call), this future never resolves.
        let mut shutdown_rx = shutdown_rx;

        // Pin the task handles so they can be selected across loop iterations
        // without being consumed.
        tokio::pin!(ipc_handle);
        tokio::pin!(tick_handle);
        tokio::pin!(snap_handle);

        // Main command loop — also watches spawned tasks for panics.
        loop {
            tokio::select! {
                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(Command::Action { conn_id, request_id, action, resp_tx }) => {
                            // Register the connection's response sender on first use.
                            self.conn_resp.entry(conn_id).or_insert_with(|| resp_tx.clone());
                            match self.handle_action(request_id, action, &resp_tx).await {
                                Ok(shutdown) => {
                                    if shutdown {
                                        break;
                                    }
                                }
                                Err(e) => {
                                    warn!(request_id, "action error: {e}");
                                }
                            }
                        }
                        Some(Command::Disconnect { conn_id }) => {
                            self.conn_resp.remove(&conn_id);
                        }
                        Some(Command::Shutdown) | None => break,
                    }
                }
                // Ctrl-C / external shutdown signal (Wave 3.1 Fix 3).
                _ = async {
                    match shutdown_rx.as_mut() {
                        Some(rx) => { let _ = rx.await; }
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    info!("shutdown signal received — flushing and exiting");
                    break;
                }
                res = &mut ipc_handle => {
                    match res {
                        Ok(_) => warn!("IPC listener task exited unexpectedly"),
                        Err(e) => error!("IPC listener task panicked: {e}; shutting down"),
                    }
                    break;
                }
                res = &mut tick_handle => {
                    match res {
                        Ok(_) => warn!("tick timer task exited unexpectedly"),
                        Err(e) => error!("tick timer task panicked: {e}; shutting down"),
                    }
                    break;
                }
                res = &mut snap_handle => {
                    match res {
                        Ok(_) => warn!("snapshot timer task exited unexpectedly"),
                        Err(e) => error!("snapshot timer task panicked: {e}; shutting down"),
                    }
                    break;
                }
            }
        }

        self.shutdown().await
    }

    /// Handle one action. Returns `true` if the daemon should shut down.
    async fn handle_action(
        &mut self,
        request_id: u64,
        action: Action,
        resp_tx: &mpsc::Sender<Message>,
    ) -> Result<bool> {
        match action {
            Action::Shutdown => {
                info!("shutdown requested");
                self.send_response(
                    resp_tx,
                    request_id,
                    ResponseResult::ok(serde_json::Value::Null),
                )
                .await;
                return Ok(true);
            }

            Action::TriggerReminderNow { reminder_id } => {
                self.run_scheduler_tick(reminder_id).await?;
                // request_id == 0 for internal ticks — no response needed.
            }

            Action::Complete { reminder_id } => {
                let now_ms = chrono::Utc::now().timestamp_millis();
                let events = self.build_complete_events(&reminder_id, now_ms).await;
                match events {
                    Ok(evs) => {
                        self.commit(evs).await?;
                        self.send_response(
                            resp_tx,
                            request_id,
                            ResponseResult::ok(serde_json::Value::Null),
                        )
                        .await;
                    }
                    Err(e) => {
                        self.send_response(resp_tx, request_id, ResponseResult::err(e.to_string()))
                            .await;
                    }
                }
            }

            Action::Snooze {
                reminder_id,
                minutes,
            } => {
                let now_ms = chrono::Utc::now().timestamp_millis();
                let until_ms = now_ms + (minutes as i64 * 60 * 1000);
                let ev = Event::ReminderSnoozed {
                    reminder_id,
                    until_ms,
                    snooze_min: minutes,
                };
                self.commit(vec![ev]).await?;
                self.send_response(
                    resp_tx,
                    request_id,
                    ResponseResult::ok(serde_json::Value::Null),
                )
                .await;
            }

            Action::TogglePin { reminder_id } => {
                let pinned = {
                    let s = self.state.read().await;
                    s.reminders
                        .get(&reminder_id)
                        .map(|rt| rt.pinned)
                        .unwrap_or(false)
                };
                let ev = if pinned {
                    Event::ReminderUnpinned { reminder_id }
                } else {
                    Event::ReminderPinned { reminder_id }
                };
                self.commit(vec![ev]).await?;
                self.send_response(
                    resp_tx,
                    request_id,
                    ResponseResult::ok(serde_json::Value::Null),
                )
                .await;
            }

            Action::SetReminderInterval {
                reminder_id,
                minutes,
            } => {
                if !(1..=24 * 60).contains(&minutes) {
                    self.send_response(
                        resp_tx,
                        request_id,
                        ResponseResult::err(format!(
                            "interval {minutes} out of range [1, {}]",
                            24 * 60
                        )),
                    )
                    .await;
                } else {
                    let ev = Event::ReminderIntervalChanged {
                        reminder_id,
                        minutes,
                    };
                    self.commit(vec![ev]).await?;
                    self.send_response(
                        resp_tx,
                        request_id,
                        ResponseResult::ok(serde_json::Value::Null),
                    )
                    .await;
                }
            }

            Action::ToggleEnabled { reminder_id } => {
                let enabled = {
                    let s = self.state.read().await;
                    s.reminders
                        .get(&reminder_id)
                        .map(|rt| rt.enabled)
                        .unwrap_or(true)
                };
                let ev = if enabled {
                    Event::ReminderDisabled { reminder_id }
                } else {
                    Event::ReminderEnabled { reminder_id }
                };
                self.commit(vec![ev]).await?;
                self.send_response(
                    resp_tx,
                    request_id,
                    ResponseResult::ok(serde_json::Value::Null),
                )
                .await;
            }

            Action::SetPalette { palette } => {
                let ev = Event::ConfigChanged {
                    key: "palette".into(),
                    value: serde_json::Value::String(palette),
                };
                self.commit(vec![ev]).await?;
                self.send_response(
                    resp_tx,
                    request_id,
                    ResponseResult::ok(serde_json::Value::Null),
                )
                .await;
            }

            Action::SetXpMultiplier { multiplier } => {
                let clamped = multiplier.clamp(1, 1000);
                let ev = Event::ConfigChanged {
                    key: "xp_multiplier".into(),
                    value: serde_json::json!(clamped),
                };
                self.commit(vec![ev]).await?;
                self.send_response(
                    resp_tx,
                    request_id,
                    ResponseResult::ok(serde_json::Value::Null),
                )
                .await;
            }

            Action::SetTraitLevel { trait_id, level } => {
                let (current, cumulative_before, tokens_spent, total_level_before) = {
                    let s = self.state.read().await;
                    let current = *s.traits.get(&trait_id).unwrap_or(&0);
                    let cumulative_before = s.cumulative_levels_gained;
                    let tokens_spent = s.tokens_spent;
                    let total_level_before: u32 = s
                        .traits
                        .values()
                        .map(|&xp| seed_core::levels::level_for_xp(xp) as u32)
                        .sum();
                    (current, cumulative_before, tokens_spent, total_level_before)
                };
                let target_xp = seed_core::xp_for_level(level);
                if target_xp != current {
                    // Always emit TraitXpChanged as the primary event.
                    let mut events = vec![Event::TraitXpChanged {
                        trait_id: trait_id.clone(),
                        delta: target_xp as i64 - current as i64,
                        new_xp: target_xp,
                    }];
                    // For level increases: emit LevelUp per boundary, FocusTokenEarned,
                    // and TierChanged. Going down emits no LevelUps (monotonic guarantee).
                    let progression = Self::build_level_progression_events(
                        &trait_id,
                        current,
                        target_xp,
                        cumulative_before,
                        tokens_spent,
                        total_level_before,
                    );
                    events.extend(progression);
                    self.commit(events).await?;
                }
                self.send_response(
                    resp_tx,
                    request_id,
                    ResponseResult::ok(serde_json::Value::Null),
                )
                .await;
            }

            Action::Integrate {
                trait_id,
                enhancement_id,
            } => {
                // 1. Trait must exist in state.
                let current_xp = {
                    let s = self.state.read().await;
                    match s.traits.get(&trait_id).copied() {
                        Some(xp) => xp,
                        None => {
                            self.send_response(
                                resp_tx,
                                request_id,
                                ResponseResult::err(format!("unknown trait: {}", trait_id.0)),
                            )
                            .await;
                            return Ok(false);
                        }
                    }
                };
                // 2. Trait must be at level 99 (XP >= xp_for_level(99)).
                let threshold = seed_core::xp_for_level(seed_core::levels::MAX_LEVEL);
                if current_xp < threshold {
                    self.send_response(
                        resp_tx,
                        request_id,
                        ResponseResult::err(format!("trait {} is not at level 99", trait_id.0)),
                    )
                    .await;
                    return Ok(false);
                }
                // 3. Compute new_integrations for the event payload.
                let new_integrations = {
                    let s = self.state.read().await;
                    s.trait_integrations
                        .get(&trait_id)
                        .copied()
                        .unwrap_or(0)
                        .saturating_add(1)
                };
                // 4. Emit + commit.
                let ev = Event::TraitIntegrated {
                    trait_id,
                    new_integrations,
                    enhancement_id,
                };
                self.commit(vec![ev]).await?;
                self.send_response(
                    resp_tx,
                    request_id,
                    ResponseResult::ok(serde_json::Value::Null),
                )
                .await;
            }

            Action::ActivateFocusPhase { pattern, traits } => {
                use seed_core::domain::CATEGORIES;
                use seed_core::tokens_available;

                // 1. Must have at least one available token.
                let available = {
                    let s = self.state.read().await;
                    tokens_available(&s)
                };
                if available == 0 {
                    self.send_response(
                        resp_tx,
                        request_id,
                        ResponseResult::err("no focus tokens available"),
                    )
                    .await;
                    return Ok(false);
                }
                // 2. traits.len() must match pattern's skill count.
                let expected = pattern.skill_count();
                if traits.len() != expected {
                    self.send_response(
                        resp_tx,
                        request_id,
                        ResponseResult::err(format!(
                            "pattern {:?} requires {} traits, got {}",
                            pattern,
                            expected,
                            traits.len()
                        )),
                    )
                    .await;
                    return Ok(false);
                }
                // 3. No duplicate trait IDs.
                {
                    let mut seen = std::collections::BTreeSet::new();
                    for t in &traits {
                        if !seen.insert(t) {
                            self.send_response(
                                resp_tx,
                                request_id,
                                ResponseResult::err("duplicate trait id in allocation"),
                            )
                            .await;
                            return Ok(false);
                        }
                    }
                }
                // 4. All trait IDs must exist in the static catalog.
                for t in &traits {
                    if !CATEGORIES.iter().any(|c| c.trait_id == t.0.as_str()) {
                        self.send_response(
                            resp_tx,
                            request_id,
                            ResponseResult::err(format!("unknown trait id: {}", t.0)),
                        )
                        .await;
                        return Ok(false);
                    }
                }
                // 5. Emit + commit.
                let ev = Event::FocusPhaseActivated { pattern, traits };
                self.commit(vec![ev]).await?;
                self.send_response(
                    resp_tx,
                    request_id,
                    ResponseResult::ok(serde_json::Value::Null),
                )
                .await;
            }

            Action::Subscribe => {
                // Send a full snapshot directly to this specific connection only
                // (not broadcast to all clients).
                let state = self.state.read().await.clone();
                let msg = Message::Snapshot {
                    state: Box::new(state),
                };
                let _ = resp_tx.send(msg).await;
            }

            Action::Reset => {
                // Wipe per-reminder and per-trait XP progress, but preserve all
                // prestige state (M6 fix). Prestige is a lifetime achievement —
                // it is monotonic non-decreasing and must survive Reset.
                //
                // Preserved across Reset:
                //   cumulative_levels_gained, tokens_spent, active_focus,
                //   trait_integrations, trait_enhancements
                //
                // Reset (returned to initial values):
                //   per-trait XP, reminder runtimes (last_done, streaks, etc.)
                let now_ms = chrono::Utc::now().timestamp_millis();
                let fresh = {
                    let old = self.state.read().await;
                    let mut s = seed_core::initial_state(now_ms);
                    // Carry prestige fields forward.
                    s.cumulative_levels_gained = old.cumulative_levels_gained;
                    s.tokens_spent = old.tokens_spent;
                    s.active_focus = old.active_focus.clone();
                    s.trait_integrations = old.trait_integrations.clone();
                    s.trait_enhancements = old.trait_enhancements.clone();
                    s
                };
                {
                    let mut state = self.state.write().await;
                    *state = fresh.clone();
                }
                let ev = seed_core::Event::CompanionAwakened {
                    glyph_seed: fresh.glyph_seed,
                };
                // Append + broadcast without going through commit's state-apply
                // (state already replaced above).
                {
                    let mut log = self.event_log.lock().await;
                    log.append(&ev)?;
                    self.events_since_snapshot += 1;
                }
                let env = seed_core::to_envelope(&ev, chrono::Utc::now());
                if self.broadcast_tx.receiver_count() > 0 {
                    let _ = self.broadcast_tx.send(vec![env]);
                }
                // Send a fresh snapshot to the requesting connection.
                let state = self.state.read().await.clone();
                let snap = Message::Snapshot {
                    state: Box::new(state),
                };
                let _ = resp_tx.send(snap).await;
                self.send_response(
                    resp_tx,
                    request_id,
                    ResponseResult::ok(serde_json::Value::Null),
                )
                .await;
                info!("companion reset by user request");
            }
        }
        Ok(false)
    }

    /// Build level-progression events given a trait's XP moving from `old_xp` to
    /// `new_xp`. Emits `LevelUp` events for every integer level boundary crossed
    /// (only on increase), `FocusTokenEarned` for every 99-multiple boundary
    /// crossed in `cumulative_levels_gained`, and `TierChanged` if the total tier
    /// changes.
    ///
    /// `cumulative_before` and `tokens_spent` are snapshotted pre-apply values so
    /// this function remains pure of any lock access. `total_level_before` is the
    /// sum of all trait levels before the XP change (used for tier computation).
    ///
    /// Returns the new events in order: [LevelUp…, FocusTokenEarned?, TierChanged?]
    fn build_level_progression_events(
        trait_id: &TraitId,
        old_xp: u64,
        new_xp: u64,
        cumulative_before: u32,
        tokens_spent: u32,
        total_level_before: u32,
    ) -> Vec<Event> {
        use seed_core::domain::{Tier, tier_for};
        use seed_core::levels::{level_for_xp, xp_for_level};

        let mut events = Vec::new();

        let old_level = level_for_xp(old_xp);
        let new_level = level_for_xp(new_xp);

        // Only emit LevelUp events when XP increases and level advances.
        if new_xp > old_xp && new_level > old_level {
            // One LevelUp per integer boundary crossed.
            for lvl in (old_level + 1)..=new_level {
                events.push(Event::LevelUp {
                    trait_id: trait_id.clone(),
                    old_level: lvl - 1,
                    new_level: lvl,
                    new_xp: xp_for_level(lvl),
                });
            }

            // Focus token boundary detection.
            let levels_gained = (new_level as u32).saturating_sub(old_level as u32);
            let new_cumulative = cumulative_before.saturating_add(levels_gained);
            let old_tokens_earned = cumulative_before / 99;
            let new_tokens_earned = new_cumulative / 99;
            if new_tokens_earned > old_tokens_earned {
                let new_balance = new_tokens_earned.saturating_sub(tokens_spent);
                events.push(Event::FocusTokenEarned { new_balance });
            }

            // Tier change detection.
            // Old tier: derived from total_level_before (before this trait's change).
            // New tier: derived from total_level_before - old_level + new_level.
            let tier_before: Tier = tier_for(total_level_before);
            let total_level_after = total_level_before
                .saturating_sub(old_level as u32)
                .saturating_add(new_level as u32);
            let tier_after: Tier = tier_for(total_level_after);
            if tier_after != tier_before {
                events.push(Event::TierChanged {
                    from: tier_before,
                    to: tier_after,
                    total_level: total_level_after,
                });
            }
        }

        events
    }

    /// Build ReminderCompleted events for a completion action.
    ///
    /// This method holds the state read-lock for the entire computation, reads
    /// all necessary values, then drops the lock before returning. It does NOT
    /// release and re-acquire the lock mid-function. The daemon is a
    /// single-writer at the `cmd_rx.recv()` entrypoint in `run_inner` — only
    /// one `handle_action` call runs at a time, so there is no interleaved write
    /// between the single read here and the subsequent `commit` call.
    async fn build_complete_events(
        &self,
        reminder_id: &ReminderId,
        now_ms: i64,
    ) -> Result<Vec<Event>> {
        use seed_core::{
            domain::{reminder_by_id, reminder_status_with_interval},
            levels::{XpRewardOpts, level_for_xp, xp_reward},
        };

        let s = self.state.read().await;

        let static_reminder = reminder_by_id(&reminder_id.0)
            .ok_or_else(|| anyhow::anyhow!("unknown reminder: {}", reminder_id.0))?;

        // Find the trait for this reminder's category.
        let trait_id = seed_core::domain::CATEGORIES
            .iter()
            .find(|c| c.id == static_reminder.cat)
            .map(|c| TraitId(c.trait_id.to_owned()))
            .ok_or_else(|| anyhow::anyhow!("no category for reminder {}", reminder_id.0))?;

        let current_xp = *s.traits.get(&trait_id).unwrap_or(&0);

        let rt = s
            .reminders
            .get(reminder_id)
            .ok_or_else(|| anyhow::anyhow!("reminder state missing: {}", reminder_id.0))?;

        // Compute timing opts from pre-completion reminder status (C2 fix).
        // last_done_ms is the value BEFORE this completion — exactly what we need.
        let status =
            reminder_status_with_interval(rt.interval_min, rt.last_done_ms, rt.enabled, now_ms);
        let xp_opts = match status.state {
            seed_core::domain::ReminderState::Overdue => XpRewardOpts {
                on_time: false,
                overdue: true,
            },
            seed_core::domain::ReminderState::Due => XpRewardOpts {
                on_time: true,
                overdue: false,
            },
            // Dormant (early completion) or Off → 0.6× late penalty.
            _ => XpRewardOpts {
                on_time: false,
                overdue: false,
            },
        };

        let streak = rt.streak + 1;

        // Snapshot token state before releasing the lock.
        let cumulative_before = s.cumulative_levels_gained;
        let tokens_spent = s.tokens_spent;
        // Snapshot total level for tier computation.
        let total_level_before: u32 = s.traits.values().map(|&xp| level_for_xp(xp) as u32).sum();

        // Pass the active focus phase so xp_reward can apply the focus multiplier.
        let xp_gained_base = xp_reward(static_reminder, xp_opts, s.active_focus.as_ref());
        let xp_gained = (xp_gained_base as u64)
            .saturating_mul(s.xp_multiplier.max(1) as u64)
            .min(u32::MAX as u64) as u32;
        let new_xp = current_xp.saturating_add(xp_gained as u64);

        drop(s); // single read — no re-acquire needed.

        let mut events = vec![Event::ReminderCompleted {
            reminder_id: reminder_id.clone(),
            xp_gained,
            trait_id: trait_id.clone(),
            new_xp,
            streak,
            // at_ms is stamped here so apply_event can update last_done_ms,
            // resetting the scheduler's due/overdue state (Wave 3.1 Fix 1).
            at_ms: now_ms,
        }];

        // Level-up, token, and tier events via shared helper.
        let progression = Self::build_level_progression_events(
            &trait_id,
            current_xp,
            new_xp,
            cumulative_before,
            tokens_spent,
            total_level_before,
        );
        events.extend(progression);

        Ok(events)
    }

    /// Commit events: apply to state, append to log, broadcast to subscribers.
    pub async fn commit(&mut self, events: Vec<Event>) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        let mut envelopes = Vec::with_capacity(events.len());
        {
            let mut state = self.state.write().await;
            let mut log = self.event_log.lock().await;
            for ev in &events {
                apply_event(&mut state, ev);
                log.append(ev)?;
                let env = to_envelope(ev, chrono::Utc::now());
                envelopes.push(env);
            }
            self.events_since_snapshot += events.len();
        }

        // Broadcast StateDiff to all subscribers.
        if self.broadcast_tx.receiver_count() > 0 {
            let _ = self.broadcast_tx.send(envelopes);
        }

        // Periodic snapshot threshold.
        if self.events_since_snapshot >= SNAPSHOT_EVENT_THRESHOLD {
            self.write_snapshot().await;
        }

        Ok(())
    }

    /// Run a scheduler tick. If `specific_id` is Some, bypass debounce + active
    /// hours and trigger that reminder immediately (TriggerReminderNow fix).
    async fn run_scheduler_tick(&mut self, specific_id: Option<ReminderId>) -> Result<()> {
        let now = chrono::Utc::now();
        let notifier = self.notifier.clone();

        if let Some(rid) = specific_id {
            // Specific reminder: bypass debounce + active hours.
            use seed_core::domain::reminder_by_id;
            let (title, body) = match reminder_by_id(&rid.0) {
                Some(r) => (format!("seed · {}", r.name), r.desc.to_owned()),
                None => {
                    warn!(reminder = %rid.0, "TriggerReminderNow: unknown reminder id");
                    return Ok(());
                }
            };
            if let Err(e) = notifier.notify(&rid, &title, &body).await {
                warn!(reminder = %rid.0, "forced notification failed: {e}");
            }
            let ev = Event::ReminderNotified {
                reminder_id: rid,
                at_ms: now.timestamp_millis(),
            };
            self.commit(vec![ev]).await?;
        } else {
            // Normal scheduler tick.
            let events = {
                let mut state = self.state.write().await;
                schedule_tick(&mut state, &self.config, notifier.as_ref(), now).await
            };
            if !events.is_empty() {
                self.commit(events).await?;
            }
        }
        Ok(())
    }

    /// Send a response to a specific connection's channel.
    /// Skips silently if request_id == 0 (internal scheduler ticks).
    async fn send_response(
        &self,
        resp_tx: &mpsc::Sender<Message>,
        request_id: u64,
        result: ResponseResult,
    ) {
        if request_id == 0 {
            return;
        }
        let msg = Message::Response {
            id: request_id,
            result,
        };
        if resp_tx.send(msg).await.is_err() {
            warn!(request_id, "response channel closed before send");
        }
    }

    async fn write_snapshot(&mut self) {
        let snap_path = snapshot_path(&self.seed_home);
        let state = self.state.read().await.clone();
        let count = self.event_log.lock().await.event_count;
        if let Err(e) = EventLog::snapshot_write(&snap_path, &state, count) {
            warn!("snapshot write failed: {e}");
        } else {
            self.events_since_snapshot = 0;
        }
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        info!("daemon shutting down");
        // Acquire the log lock to ensure any in-flight appends complete.
        let _ = self.event_log.lock().await;
        // Write final snapshot.
        self.write_snapshot().await;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Unit tests for prestige action handlers
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::MockNotifier;
    use seed_core::{
        domain::{FocusPattern, IntegrationEnhancement, Tier},
        events::tokens_available,
        initial_state, xp_for_level,
    };
    use tokio::sync::mpsc;

    /// Build a minimal in-memory `Daemon` backed by a leaked temp directory.
    ///
    /// The temp dir is intentionally leaked (`std::mem::forget`) so the event
    /// log file stays on disk for the daemon's lifetime. This is acceptable
    /// in tests; the OS cleans up on process exit.
    async fn make_daemon(state: seed_core::State) -> Daemon {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let seed_home = tmp.path().to_path_buf();
        std::fs::create_dir_all(&seed_home).unwrap();

        let log_path = seed_core::events_path(&seed_home);
        let event_log = EventLog::open(&log_path).expect("open event log");

        let (broadcast_tx, _) = broadcast::channel(64);
        let (_cmd_tx, cmd_rx) = mpsc::channel(64);

        // Leak the temp dir so the backing file remains valid.
        std::mem::forget(tmp);

        Daemon {
            seed_home,
            state: Arc::new(RwLock::new(state)),
            event_log: Arc::new(Mutex::new(event_log)),
            config: seed_core::Config::default(),
            notifier: Arc::new(MockNotifier::new()),
            broadcast_tx,
            cmd_rx,
            events_since_snapshot: 0,
            conn_resp: HashMap::new(),
        }
    }

    /// Convenience: send an action and return the ResponseResult.
    async fn send_action(daemon: &mut Daemon, action: Action) -> ResponseResult {
        let (resp_tx, mut resp_rx) = mpsc::channel::<Message>(16);
        daemon
            .handle_action(1, action, &resp_tx)
            .await
            .expect("handle_action should not return Err");
        match resp_rx.recv().await.expect("response expected") {
            Message::Response { id: 1, result } => result,
            other => panic!("unexpected message: {other:?}"),
        }
    }

    fn flow_id() -> TraitId {
        TraitId("flow".into())
    }

    // -----------------------------------------------------------------------
    // Integrate tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn integrate_action_at_99_resets_xp_and_increments_count() {
        let mut state = initial_state(0);
        *state.traits.get_mut(&flow_id()).unwrap() = xp_for_level(99);

        let mut daemon = make_daemon(state).await;

        let result = send_action(
            &mut daemon,
            Action::Integrate {
                trait_id: flow_id(),
                enhancement_id: IntegrationEnhancement::FlowSpiral,
            },
        )
        .await;

        assert!(
            matches!(result, ResponseResult::Ok { .. }),
            "expected Ok, got {result:?}"
        );

        let s = daemon.state.read().await;
        assert_eq!(
            *s.traits.get(&flow_id()).unwrap(),
            0,
            "XP should reset to 0 after integrate"
        );
        assert_eq!(
            s.trait_integrations.get(&flow_id()).copied().unwrap_or(0),
            1,
            "integrations counter should be 1"
        );
        let enhancements = s
            .trait_enhancements
            .get(&flow_id())
            .cloned()
            .unwrap_or_default();
        assert_eq!(enhancements, vec![IntegrationEnhancement::FlowSpiral]);
    }

    #[tokio::test]
    async fn integrate_action_below_99_returns_error() {
        let mut state = initial_state(0);
        *state.traits.get_mut(&flow_id()).unwrap() = xp_for_level(98);

        let mut daemon = make_daemon(state).await;

        let result = send_action(
            &mut daemon,
            Action::Integrate {
                trait_id: flow_id(),
                enhancement_id: IntegrationEnhancement::FlowSpiral,
            },
        )
        .await;

        assert!(
            matches!(result, ResponseResult::Err { .. }),
            "expected Err for below-99 integrate, got {result:?}"
        );

        // State unchanged: XP still at xp_for_level(98), integrations still 0.
        let s = daemon.state.read().await;
        assert_eq!(
            *s.traits.get(&flow_id()).unwrap(),
            xp_for_level(98),
            "XP should be unchanged after rejection"
        );
        assert_eq!(
            s.trait_integrations.get(&flow_id()).copied().unwrap_or(0),
            0
        );
    }

    #[tokio::test]
    async fn integrate_action_unknown_trait_returns_error() {
        let mut daemon = make_daemon(initial_state(0)).await;

        let result = send_action(
            &mut daemon,
            Action::Integrate {
                trait_id: TraitId("nonexistent".into()),
                enhancement_id: IntegrationEnhancement::FlowSpiral,
            },
        )
        .await;

        assert!(
            matches!(result, ResponseResult::Err { message } if message.contains("unknown trait")),
            "expected 'unknown trait' error"
        );
    }

    #[tokio::test]
    async fn integrate_action_second_time_stacks_enhancements() {
        let mut state = initial_state(0);
        *state.traits.get_mut(&flow_id()).unwrap() = xp_for_level(99);

        let mut daemon = make_daemon(state).await;

        // First integration.
        let r1 = send_action(
            &mut daemon,
            Action::Integrate {
                trait_id: flow_id(),
                enhancement_id: IntegrationEnhancement::FlowSpiral,
            },
        )
        .await;
        assert!(
            matches!(r1, ResponseResult::Ok { .. }),
            "first integrate should succeed"
        );

        // Manually reset XP back to level 99 for the second integration.
        {
            let mut s = daemon.state.write().await;
            *s.traits.get_mut(&flow_id()).unwrap() = xp_for_level(99);
        }

        // Second integration with same enhancement.
        let r2 = send_action(
            &mut daemon,
            Action::Integrate {
                trait_id: flow_id(),
                enhancement_id: IntegrationEnhancement::FlowSpiral,
            },
        )
        .await;
        assert!(
            matches!(r2, ResponseResult::Ok { .. }),
            "second integrate should succeed"
        );

        let s = daemon.state.read().await;
        assert_eq!(
            s.trait_integrations.get(&flow_id()).copied().unwrap_or(0),
            2
        );
        let enhancements = s
            .trait_enhancements
            .get(&flow_id())
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            enhancements,
            vec![
                IntegrationEnhancement::FlowSpiral,
                IntegrationEnhancement::FlowSpiral
            ]
        );
    }

    // -----------------------------------------------------------------------
    // ActivateFocusPhase tests
    // -----------------------------------------------------------------------

    /// Build a state with 1 token available (cumulative_levels_gained = 99).
    fn state_with_tokens(n: u32) -> seed_core::State {
        let mut s = initial_state(0);
        s.cumulative_levels_gained = 99 * n;
        s
    }

    #[tokio::test]
    async fn activate_focus_with_token_emits_event() {
        let mut daemon = make_daemon(state_with_tokens(1)).await;

        let result = send_action(
            &mut daemon,
            Action::ActivateFocusPhase {
                pattern: FocusPattern::Spread3x2,
                traits: vec![
                    TraitId("flow".into()),
                    TraitId("core".into()),
                    TraitId("spine".into()),
                ],
            },
        )
        .await;

        assert!(
            matches!(result, ResponseResult::Ok { .. }),
            "expected Ok, got {result:?}"
        );

        let s = daemon.state.read().await;
        assert_eq!(s.tokens_spent, 1, "one token should have been spent");
        assert!(
            s.active_focus.is_some(),
            "active_focus should be Some after activation"
        );
        let focus = s.active_focus.as_ref().unwrap();
        assert_eq!(
            focus.allocations.len(),
            3,
            "Spread3x2 should have 3 allocations"
        );
    }

    #[tokio::test]
    async fn activate_focus_without_token_returns_error() {
        let mut state = initial_state(0);
        state.cumulative_levels_gained = 0;

        let mut daemon = make_daemon(state).await;

        let result = send_action(
            &mut daemon,
            Action::ActivateFocusPhase {
                pattern: FocusPattern::Spread3x2,
                traits: vec![
                    TraitId("flow".into()),
                    TraitId("core".into()),
                    TraitId("spine".into()),
                ],
            },
        )
        .await;

        assert!(
            matches!(result, ResponseResult::Err { .. }),
            "expected Err when no tokens available"
        );

        let s = daemon.state.read().await;
        assert_eq!(s.tokens_spent, 0);
        assert!(s.active_focus.is_none());
    }

    #[tokio::test]
    async fn activate_focus_pattern_length_mismatch_returns_error() {
        let mut daemon = make_daemon(state_with_tokens(1)).await;

        // Spread3x2 requires 3 traits; provide only 2.
        let result = send_action(
            &mut daemon,
            Action::ActivateFocusPhase {
                pattern: FocusPattern::Spread3x2,
                traits: vec![TraitId("flow".into()), TraitId("core".into())],
            },
        )
        .await;

        assert!(
            matches!(result, ResponseResult::Err { message } if message.contains("requires")),
            "expected pattern-mismatch error"
        );

        // Token was not consumed.
        let s = daemon.state.read().await;
        assert_eq!(s.tokens_spent, 0);
    }

    #[tokio::test]
    async fn activate_focus_duplicate_traits_returns_error() {
        let mut daemon = make_daemon(state_with_tokens(1)).await;

        // Spread2x3 requires 2 traits; provide [flow, flow].
        let result = send_action(
            &mut daemon,
            Action::ActivateFocusPhase {
                pattern: FocusPattern::Spread2x3,
                traits: vec![TraitId("flow".into()), TraitId("flow".into())],
            },
        )
        .await;

        assert!(
            matches!(result, ResponseResult::Err { message } if message.contains("duplicate")),
            "expected duplicate trait error"
        );

        let s = daemon.state.read().await;
        assert_eq!(s.tokens_spent, 0);
    }

    #[tokio::test]
    async fn activate_focus_unknown_trait_returns_error() {
        let mut daemon = make_daemon(state_with_tokens(1)).await;

        // Spread2x3 requires 2 traits; provide [flow, nonexistent].
        let result = send_action(
            &mut daemon,
            Action::ActivateFocusPhase {
                pattern: FocusPattern::Spread2x3,
                traits: vec![TraitId("flow".into()), TraitId("nonexistent".into())],
            },
        )
        .await;

        assert!(
            matches!(result, ResponseResult::Err { message } if message.contains("unknown trait id")),
            "expected unknown-trait error"
        );

        let s = daemon.state.read().await;
        assert_eq!(s.tokens_spent, 0);
    }

    #[tokio::test]
    async fn activate_focus_replaces_prior_phase() {
        // 2 tokens.
        let mut daemon = make_daemon(state_with_tokens(2)).await;

        // First activation: Concentrate1x4 on flow.
        let r1 = send_action(
            &mut daemon,
            Action::ActivateFocusPhase {
                pattern: FocusPattern::Concentrate1x4,
                traits: vec![TraitId("flow".into())],
            },
        )
        .await;
        assert!(
            matches!(r1, ResponseResult::Ok { .. }),
            "first activation should succeed"
        );

        // Second activation: Spread2x3 on core + spine.
        let r2 = send_action(
            &mut daemon,
            Action::ActivateFocusPhase {
                pattern: FocusPattern::Spread2x3,
                traits: vec![TraitId("core".into()), TraitId("spine".into())],
            },
        )
        .await;
        assert!(
            matches!(r2, ResponseResult::Ok { .. }),
            "second activation should succeed"
        );

        let s = daemon.state.read().await;
        assert_eq!(s.tokens_spent, 2, "two tokens should have been spent");
        let focus = s
            .active_focus
            .as_ref()
            .expect("active_focus should be Some");
        assert_eq!(
            focus.pattern,
            FocusPattern::Spread2x3,
            "second pattern should be active"
        );
        assert_eq!(focus.allocations.len(), 2, "Spread2x3 allocates 2 traits");
    }

    // -----------------------------------------------------------------------
    // build_level_progression_events unit tests (A2)
    // -----------------------------------------------------------------------

    #[test]
    fn build_level_progression_events_unit_no_change() {
        let trait_id = flow_id();
        let xp = seed_core::xp_for_level(5);
        let evs = Daemon::build_level_progression_events(&trait_id, xp, xp, 0, 0, 0);
        assert!(evs.is_empty(), "no XP change → no events");
    }

    #[test]
    fn build_level_progression_events_unit_decrease_no_levelups() {
        let trait_id = flow_id();
        let old_xp = seed_core::xp_for_level(50);
        let new_xp = seed_core::xp_for_level(10);
        let evs = Daemon::build_level_progression_events(&trait_id, old_xp, new_xp, 0, 0, 400);
        // Decrease: build_level_progression_events only emits on increase.
        assert!(evs.is_empty(), "XP decrease → no events");
    }

    #[test]
    fn build_level_progression_events_unit_single_levelup() {
        let trait_id = flow_id();
        let old_xp = seed_core::xp_for_level(4);
        let new_xp = seed_core::xp_for_level(5);
        let evs = Daemon::build_level_progression_events(&trait_id, old_xp, new_xp, 0, 0, 30);
        assert_eq!(evs.len(), 1, "one level boundary → one LevelUp");
        assert!(
            matches!(&evs[0], Event::LevelUp { new_level: 5, .. }),
            "expected LevelUp to level 5"
        );
    }

    #[test]
    fn build_level_progression_events_unit_multi_levelup() {
        let trait_id = flow_id();
        let old_xp = seed_core::xp_for_level(1);
        let new_xp = seed_core::xp_for_level(50);
        let evs = Daemon::build_level_progression_events(&trait_id, old_xp, new_xp, 0, 0, 0);
        // 49 LevelUp events (levels 2..=50), no token (cumulative 49 < 99), may have TierChanged.
        let levelup_count = evs
            .iter()
            .filter(|e| matches!(e, Event::LevelUp { .. }))
            .count();
        assert_eq!(levelup_count, 49, "1→50 = 49 level boundaries");
    }

    #[test]
    fn build_level_progression_events_unit_token_at_boundary() {
        let trait_id = flow_id();
        // level 1 → 99 = 98 level-up crossings.
        // With cumulative_before=1, total cumulative = 1+98 = 99 → crosses 99-boundary → 1 token.
        let old_xp = seed_core::xp_for_level(1);
        let new_xp = seed_core::xp_for_level(99);
        let evs = Daemon::build_level_progression_events(&trait_id, old_xp, new_xp, 1, 0, 0);
        let token_events: Vec<_> = evs
            .iter()
            .filter(|e| matches!(e, Event::FocusTokenEarned { .. }))
            .collect();
        assert_eq!(
            token_events.len(),
            1,
            "should earn exactly one token at 99 boundary"
        );
        if let Event::FocusTokenEarned { new_balance } = token_events[0] {
            assert_eq!(*new_balance, 1, "first token → balance 1");
        }
    }

    #[test]
    fn build_level_progression_events_unit_multi_token_cross() {
        // cumulative_before = 97, tokens_spent = 0.
        // Gain 99 levels (lvl 1→99+1 is 99 levels including level 0 base, but let's be concrete):
        // xp from lvl 1 to lvl 99 = 98 new levels.
        // With cumulative_before=98 → new_cumulative = 98+98 = 196.
        // Tokens earned: 196/99 = 1 vs 98/99 = 0 → 1 new token.
        // Multi-cross scenario: cumulative_before = 1, gain 200 levels (impossible in one trait
        // but the helper is pure, so we can craft the input).
        // Use build_level_progression_events directly with crafted old/new levels via XP.
        // Level 1 → 99 from cumulative_before=2: cumulative = 2+98 = 100.
        // 100/99 = 1 (floor), 2/99 = 0 (floor) → 1 token. Just 1.
        // To force 2 tokens in one call, use level 1→99 with cumulative_before = 0+99+1 = 100-ish.
        // Actually: need gains >= 198 to cross two 99-multiples starting from 0.
        // That requires 2 traits. The helper is per-trait, so use extreme: lvl 1→99 twice
        // would be two calls. For a single call crossing 2 tokens, we'd need cumulative_before
        // such that cumulative_before % 99 + levels_gained >= 198.
        // Simple: cumulative_before = 98 (one below first boundary), levels = 100 (impossible in
        // one trait; max is 98 per trait lvl 1→99). So we can't cross 2 in one trait call.
        // Instead, verify the single-call with cumulative_before crossing exactly one boundary
        // correctly computes balance and emits only one FocusTokenEarned.
        // (Multi-token-cross in a single helper call is mathematically impossible for any single
        // trait since max levels_gained per call is 98. The spec says one per 99 cumulative so
        // max one FocusTokenEarned per build_level_progression_events call.)
        // This test verifies: no-token below boundary, exactly-one at boundary, increase case.
        let trait_id = flow_id();
        let old_xp = seed_core::xp_for_level(1);
        let new_xp = seed_core::xp_for_level(99);
        // cumulative_before = 0, levels_gained = 98 → new_cumulative = 98 → 98/99 = 0. No token.
        let evs_no_token =
            Daemon::build_level_progression_events(&trait_id, old_xp, new_xp, 0, 0, 0);
        let token_count_no = evs_no_token
            .iter()
            .filter(|e| matches!(e, Event::FocusTokenEarned { .. }))
            .count();
        assert_eq!(
            token_count_no, 0,
            "98 cumulative levels → no token (below 99)"
        );

        // cumulative_before = 2, levels_gained = 98 → new_cumulative = 100 → 1 token earned.
        // tokens_spent = 0 → new_balance = 1.
        let evs_one = Daemon::build_level_progression_events(&trait_id, old_xp, new_xp, 2, 0, 0);
        let token_events_one: Vec<_> = evs_one
            .iter()
            .filter(|e| matches!(e, Event::FocusTokenEarned { .. }))
            .collect();
        assert_eq!(
            token_events_one.len(),
            1,
            "crossing 99 boundary → one FocusTokenEarned"
        );
        if let Event::FocusTokenEarned { new_balance } = token_events_one[0] {
            assert_eq!(*new_balance, 1);
        }

        // cumulative_before = 2, tokens_spent = 1 → balance = earned - spent.
        // 100/99 = 1 earned; 1 - 1 spent = 0 available. But token event still fires.
        // new_balance in the event = new_tokens_earned - tokens_spent = 1 - 1 = 0.
        let evs_spent = Daemon::build_level_progression_events(&trait_id, old_xp, new_xp, 2, 1, 0);
        let token_events_spent: Vec<_> = evs_spent
            .iter()
            .filter(|e| matches!(e, Event::FocusTokenEarned { .. }))
            .collect();
        assert_eq!(
            token_events_spent.len(),
            1,
            "token fires even when already spent"
        );
        if let Event::FocusTokenEarned { new_balance } = token_events_spent[0] {
            // new_balance = new_tokens_earned (1) - tokens_spent (1) = 0.
            assert_eq!(
                *new_balance, 0,
                "balance = 0 when earned token was already spent"
            );
        }
    }

    // -----------------------------------------------------------------------
    // SetTraitLevel progression tests (A2)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn set_trait_level_emits_levelups_progressively() {
        let mut daemon = make_daemon(initial_state(0)).await;
        // Set flow from level 1 (xp=0) to level 50.
        let result = send_action(
            &mut daemon,
            Action::SetTraitLevel {
                trait_id: flow_id(),
                level: 50,
            },
        )
        .await;
        assert!(matches!(result, ResponseResult::Ok { .. }));

        // Read the committed events from the event log.
        let log = daemon.event_log.lock().await;
        let evs =
            EventLog::load_from(&seed_core::events_path(&daemon.seed_home), 0).unwrap_or_default();
        drop(log);

        let levelup_count = evs
            .iter()
            .filter(|e| matches!(e, Event::LevelUp { .. }))
            .count();
        assert_eq!(
            levelup_count, 49,
            "lvl 1 → 50 should emit 49 LevelUp events"
        );
    }

    #[tokio::test]
    async fn set_trait_level_emits_focus_token_at_99_boundary() {
        // Set flow from level 1 to level 99: 98 LevelUps, cumulative_levels_gained goes from 0 to 98.
        // 98 < 99, no token yet. Need to go past 99 cumulative. Use all traits.
        // Easier: set cumulative_before = 1 via pre-seeded state.
        let mut state = initial_state(0);
        state.cumulative_levels_gained = 1; // already has 1 level-up in history
        let mut daemon = make_daemon(state).await;

        // Set flow 1→99: 98 new LevelUps → cumulative goes 1+98=99 → crosses 99-boundary → token.
        let result = send_action(
            &mut daemon,
            Action::SetTraitLevel {
                trait_id: flow_id(),
                level: 99,
            },
        )
        .await;
        assert!(matches!(result, ResponseResult::Ok { .. }));

        let s = daemon.state.read().await;
        let balance = tokens_available(&s);
        assert!(
            balance >= 1,
            "should have earned at least 1 focus token, got {balance}"
        );
    }

    #[tokio::test]
    async fn set_trait_level_down_emits_no_levelups() {
        let mut state = initial_state(0);
        *state.traits.get_mut(&flow_id()).unwrap() = seed_core::xp_for_level(50);
        let mut daemon = make_daemon(state).await;

        let result = send_action(
            &mut daemon,
            Action::SetTraitLevel {
                trait_id: flow_id(),
                level: 10,
            },
        )
        .await;
        assert!(matches!(result, ResponseResult::Ok { .. }));

        let evs =
            EventLog::load_from(&seed_core::events_path(&daemon.seed_home), 0).unwrap_or_default();

        let levelup_count = evs
            .iter()
            .filter(|e| matches!(e, Event::LevelUp { .. }))
            .count();
        assert_eq!(levelup_count, 0, "XP decrease emits no LevelUp events");
    }

    // -----------------------------------------------------------------------
    // TierChanged emission tests (A1)
    // -----------------------------------------------------------------------

    #[test]
    fn tier_changed_not_emitted_when_unchanged() {
        // Start well within Seed tier (total level < 18).
        // xp=0 → all lvl 1 → total=9. LevelUp from 1→2 keeps total at 10 → still Seed (need 18 for Sprout).
        let old_xp = seed_core::xp_for_level(1);
        let new_xp = seed_core::xp_for_level(2);
        let evs = Daemon::build_level_progression_events(&flow_id(), old_xp, new_xp, 0, 0, 9);
        let tier_events: Vec<_> = evs
            .iter()
            .filter(|e| matches!(e, Event::TierChanged { .. }))
            .collect();
        assert!(
            tier_events.is_empty(),
            "no tier change within Seed band; got {tier_events:?}"
        );
    }

    #[test]
    fn tier_changed_emitted_when_complete_crosses_tier() {
        // Build a state where total level is just below Sprout threshold (18).
        // If flow goes from 1→2 and total was 17, new total = 18 → Sprout.
        let old_xp = seed_core::xp_for_level(1);
        let new_xp = seed_core::xp_for_level(2);
        let total_level_before = 17u32; // one below Sprout threshold
        let evs = Daemon::build_level_progression_events(
            &flow_id(),
            old_xp,
            new_xp,
            0,
            0,
            total_level_before,
        );
        let tier_events: Vec<_> = evs
            .iter()
            .filter(|e| matches!(e, Event::TierChanged { .. }))
            .collect();
        assert_eq!(
            tier_events.len(),
            1,
            "should emit TierChanged at tier boundary"
        );
        if let Event::TierChanged {
            from,
            to,
            total_level,
        } = tier_events[0]
        {
            assert_eq!(*from, Tier::Seed);
            assert_eq!(*to, Tier::Sprout);
            assert_eq!(*total_level, 18);
        }
    }

    #[tokio::test]
    async fn set_trait_level_emits_tier_changed_at_boundary() {
        // Build a state with total=17 (all traits at 1 except flow, which we'll bump).
        let state = initial_state(0);
        // Set all non-flow traits to lvl 1 (already 0 xp = lvl 1). Set flow to lvl 10,
        // so total=10+8=18 which is already Sprout. Let's try a different approach:
        // have all at level 2 = total 18, then bump one from 1→2 to tip from Seed.
        // Actually simplest: keep all at lvl 1 (default), put total_level = 9.
        // Flow going 1→10 → total goes 9 → 18 = Sprout threshold.
        let mut daemon = make_daemon(state).await;

        let result = send_action(
            &mut daemon,
            Action::SetTraitLevel {
                trait_id: flow_id(),
                level: 10, // lvl 1 → 10 on flow, total goes 9 → 18 (Sprout)
            },
        )
        .await;
        assert!(matches!(result, ResponseResult::Ok { .. }));

        let evs =
            EventLog::load_from(&seed_core::events_path(&daemon.seed_home), 0).unwrap_or_default();

        let tier_events: Vec<_> = evs
            .iter()
            .filter(|e| matches!(e, Event::TierChanged { .. }))
            .collect();
        assert!(
            !tier_events.is_empty(),
            "should emit at least one TierChanged when crossing Seed→Sprout"
        );
    }
}
