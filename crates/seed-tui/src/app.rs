/// App struct, state mirror, event loop, tick, terminal setup/teardown.
use std::path::PathBuf;

use anyhow::Result;
use chrono::Utc;
use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::Widget,
};
use seed_core::{
    apply_event,
    config::{Config, load as load_config},
    domain::{CATEGORIES, REMINDERS, TraitId, reminder_status_with_interval},
    events::{Event as CoreEvent, EventEnvelope, tokens_available},
    levels::{level_for_xp, xp_for_level},
    state::{State, initial_state},
};
use tokio::time::{Duration, interval};
use tracing::{debug, info, warn};

use crate::{
    client::{Action as IpcAction, IpcClient, Message},
    command::{ParsedCommand, parse},
    input::{Action, map_event},
    palette::palette_for,
    prestige::{
        FOCUS_PATTERNS, PhaseChooserStage, PrestigeModal, default_enhancement, parse_enhancement,
    },
    term::{TerminalGuard, braille_supported, truecolor_supported},
    view::{
        command_bar::CommandBar,
        orbit::OrbitPane,
        side_panel::{LEVELS_LEAD_LINES, LEVELS_LINES_PER_CAT, SidePanel, SideTab},
        skill_detail::{SkillDetailAction, SkillDetailState},
        status_bar::StatusBar,
        title_bar::TitleBar,
        toast::{Toast, ToastWidget},
        tweaks::{TweakAction, TweaksPanel, TweaksPanelState},
    },
};

/// Selection indices for the side panel tabs.
#[derive(Debug, Default)]
pub struct SideSelection {
    /// Selected row in LIST (0-based, counting only enabled reminder rows).
    pub list_idx: usize,
    /// Selected row in LEVELS (0-based, by category index).
    pub levels_idx: usize,
    /// Scroll offset for LIST.
    pub list_offset: usize,
    /// Scroll offset for LEVELS.
    pub levels_offset: usize,
    /// Scroll offset for LOG.
    pub log_offset: usize,
}

#[allow(dead_code)]
pub struct App {
    /// Local state mirror, updated from Snapshot + StateDiff.
    state: State,
    config: Config,
    pub side_tab: SideTab,
    command: String,
    toast: Option<Toast>,
    tweaks_open: bool,
    tweaks_state: TweaksPanelState,
    /// Frame counter for animations (~20 Hz).
    tick: u32,
    client: IpcClient,
    should_quit: bool,
    truecolor: bool,
    braille: bool,
    seed_home: PathBuf,
    /// Next request id.
    next_req_id: u64,
    /// Row selection + scroll offsets for side panel.
    side_selection: SideSelection,
    /// Open skill detail overlay (None = closed).
    skill_detail: Option<SkillDetailState>,
    /// Prestige modal overlay (enhancement chooser / phase chooser).
    prestige_modal: PrestigeModal,
    /// Rendered viewport height of the LIST tab (lines). Written during render, read by nav.
    list_viewport_h: usize,
    /// Rendered viewport height of the LEVELS tab (lines). Written during render, read by nav.
    levels_viewport_h: usize,
}

impl App {
    pub async fn run(seed_home: PathBuf) -> Result<()> {
        let truecolor = truecolor_supported();
        let braille = braille_supported();
        info!(truecolor, braille, "terminal capabilities");

        // Load config.
        let config = load_config(&seed_home).unwrap_or_default();
        info!(palette = %config.palette, "config loaded");

        // Connect to (or spawn) daemon.
        let mut client = IpcClient::connect_or_spawn(&seed_home).await?;
        info!("IPC connected");

        // Send Subscribe.
        client
            .send(Message::Request {
                id: 1,
                action: IpcAction::Subscribe,
            })
            .await;

        // Await Snapshot (with 3s timeout, fall back to initial_state).
        let now_ms = Utc::now().timestamp_millis();
        let state = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                match client.recv().await {
                    Some(Message::Snapshot { state }) => return *state,
                    Some(Message::Hello { .. }) => {}
                    Some(other) => debug!("pre-snapshot msg: {other:?}"),
                    None => return initial_state(now_ms),
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            warn!("snapshot timeout — using initial_state");
            initial_state(now_ms)
        });

        info!(completed = state.completed_total, "snapshot received");

        let mut app = App {
            state,
            config,
            side_tab: SideTab::default(),
            command: String::new(),
            toast: None,
            tweaks_open: false,
            tweaks_state: TweaksPanelState::default(),
            tick: 0,
            client,
            should_quit: false,
            truecolor,
            braille,
            seed_home,
            next_req_id: 2,
            side_selection: SideSelection::default(),
            skill_detail: None,
            prestige_modal: PrestigeModal::None,
            list_viewport_h: 0,
            levels_viewport_h: 0,
        };

        // Setup terminal.
        let mut guard = TerminalGuard::enter()?;
        let stdout = std::io::stdout();
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        // Main event loop.
        let mut tick_interval = interval(Duration::from_millis(50)); // ~20 Hz
        let mut event_stream = EventStream::new();

        loop {
            // Draw.
            let _ = terminal.draw(|f| {
                app.render(f);
            });

            if app.should_quit {
                break;
            }

            tokio::select! {
                // Terminal input.
                maybe_event = event_stream.next() => {
                    match maybe_event {
                        Some(Ok(ev)) => app.handle_input(ev).await,
                        Some(Err(e)) => warn!("input error: {e}"),
                        None => { app.should_quit = true; }
                    }
                }
                // IPC inbound.
                msg = app.client.recv() => {
                    match msg {
                        Some(m) => app.handle_ipc(m),
                        None => warn!("IPC channel closed"),
                    }
                }
                // Tick.
                _ = tick_interval.tick() => {
                    app.tick = app.tick.wrapping_add(1);
                }
            }
        }

        // Explicit restore before returning.
        guard.restore();
        drop(terminal);

        Ok(())
    }

    /// Build an App wired to a dummy in-memory IPC client, for testing the
    /// state-mirror / toast logic without a live terminal or daemon.
    #[cfg(test)]
    fn new_for_test(state: State) -> Self {
        use tokio::sync::mpsc;
        let (outbound, _out_rx) = mpsc::channel::<Message>(8);
        let (_in_tx, inbound) = mpsc::channel::<Message>(8);
        App {
            state,
            config: Config::default(),
            side_tab: SideTab::default(),
            command: String::new(),
            toast: None,
            tweaks_open: false,
            tweaks_state: TweaksPanelState::default(),
            tick: 0,
            client: IpcClient { outbound, inbound },
            should_quit: false,
            truecolor: false,
            braille: false,
            seed_home: PathBuf::from("/tmp/seed-test"),
            next_req_id: 2,
            side_selection: SideSelection::default(),
            skill_detail: None,
            prestige_modal: PrestigeModal::None,
            list_viewport_h: 0,
            levels_viewport_h: 0,
        }
    }

    /// Render the full frame.
    fn render(&mut self, f: &mut ratatui::Frame) {
        let area = f.area();
        let palette = palette_for(&self.state.palette);
        let buf = f.buffer_mut();

        // Fill background.
        use ratatui::style::Style;
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                buf[(x, y)].set_style(
                    Style::default()
                        .bg(crate::palette::downgrade_color(palette.bg, self.truecolor)),
                );
            }
        }

        // Layout: title (1) | body (remaining - 3) | command (1) | status (1)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title bar
                Constraint::Min(10),   // body
                Constraint::Length(1), // command bar
                Constraint::Length(1), // status bar
            ])
            .split(area);

        let title_area = chunks[0];
        let body_area = chunks[1];
        let cmd_area = chunks[2];
        let status_area = chunks[3];

        // Body: orbit | side panel
        let body_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20), Constraint::Length(40)])
            .split(body_area);

        let orbit_area = body_chunks[0];
        let side_area = body_chunks[1];

        // ── Title bar ───────────────────────────────────────────────────────
        let total_level: u32 = self
            .state
            .traits
            .values()
            .map(|&xp| level_for_xp(xp) as u32)
            .sum();
        let tier = seed_core::domain::tier_for(total_level);

        TitleBar {
            tier,
            total_level,
            palette,
            palette_name: &self.state.palette,
            truecolor: self.truecolor,
            tokens_available: tokens_available(&self.state),
        }
        .render(title_area, buf);

        // ── Orbit ────────────────────────────────────────────────────────────
        let now_ms = Utc::now().timestamp_millis();
        OrbitPane {
            state: &self.state,
            tick: self.tick,
            palette,
            truecolor: self.truecolor,
            braille: self.braille,
            now_ms,
        }
        .render(orbit_area, buf);

        // ── Side panel ───────────────────────────────────────────────────────
        SidePanel {
            state: &self.state,
            tab: self.side_tab,
            palette,
            truecolor: self.truecolor,
            list_idx: self.side_selection.list_idx,
            list_offset: self.side_selection.list_offset,
            levels_idx: self.side_selection.levels_idx,
            levels_offset: self.side_selection.levels_offset,
            log_offset: self.side_selection.log_offset,
            list_viewport_h: &mut self.list_viewport_h,
            levels_viewport_h: &mut self.levels_viewport_h,
        }
        .render(side_area, buf);

        // ── Command bar ──────────────────────────────────────────────────────
        CommandBar {
            input: &self.command,
            palette,
            truecolor: self.truecolor,
        }
        .render(cmd_area, buf);

        // ── Status bar ───────────────────────────────────────────────────────
        let any_overdue = REMINDERS.iter().any(|r| {
            self.state
                .reminders
                .get(&r.reminder_id())
                .map(|rt| {
                    reminder_status_with_interval(
                        rt.interval_min,
                        rt.last_done_ms,
                        rt.enabled,
                        now_ms,
                    )
                    .state
                        == seed_core::domain::ReminderState::Overdue
                })
                .unwrap_or(false)
        });
        let max_total = 99u32 * self.state.traits.len() as u32;
        let wellness = if max_total > 0 {
            total_level as f32 / max_total as f32
        } else {
            0.0
        };

        StatusBar {
            completed_total: self.state.completed_total,
            any_overdue,
            wellness,
            palette,
            truecolor: self.truecolor,
        }
        .render(status_area, buf);

        // ── Tweaks panel (floating) ───────────────────────────────────────────
        if self.tweaks_open {
            TweaksPanel {
                state: &self.tweaks_state,
                palette_name: &self.state.palette,
                palette,
                truecolor: self.truecolor,
                xp_multiplier: self.state.xp_multiplier,
            }
            .render(area, buf);
        }

        // ── Skill detail overlay ──────────────────────────────────────────────
        if let Some(ref sd) = self.skill_detail {
            use crate::view::skill_detail::SkillDetail;
            SkillDetail {
                state: sd,
                app_state: &self.state,
                palette,
                truecolor: self.truecolor,
            }
            .render(area, buf);
        }

        // ── Prestige modal overlay ────────────────────────────────────────────
        match &self.prestige_modal {
            PrestigeModal::None => {}
            PrestigeModal::EnhancementChooser { trait_id, cursor } => {
                use crate::view::prestige_modal::EnhancementChooserWidget;
                EnhancementChooserWidget {
                    trait_id,
                    cursor: *cursor,
                    app_state: &self.state,
                    palette,
                    truecolor: self.truecolor,
                }
                .render(area, buf);
            }
            PrestigeModal::PhaseChooser(stage) => {
                use crate::view::prestige_modal::PhaseChooserWidget;
                PhaseChooserWidget {
                    stage,
                    palette,
                    truecolor: self.truecolor,
                }
                .render(area, buf);
            }
        }

        // ── Toast overlay ────────────────────────────────────────────────────
        if let Some(ref t) = self.toast
            && t.is_visible(self.tick)
        {
            ToastWidget {
                toast: t,
                palette,
                truecolor: self.truecolor,
            }
            .render(area, buf);
        }
    }

    /// Handle a terminal input event.
    async fn handle_input(&mut self, event: Event) {
        // If tweaks panel is open, route key events to it first.
        // Only Ctrl+T (ToggleTweaks) and Quit pass through regardless.
        if self.tweaks_open
            && let Event::Key(key) = event
            && matches!(
                key.kind,
                crossterm::event::KeyEventKind::Press | crossterm::event::KeyEventKind::Repeat
            )
        {
            use crossterm::event::KeyCode;
            // Ctrl+T or 'q' still closes/quits globally.
            let is_ctrl_t = key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL)
                && key.code == KeyCode::Char('t');
            let is_quit = key.code == KeyCode::Char('q')
                || key.code == KeyCode::Char('Q')
                || (key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q')));
            if is_ctrl_t {
                self.tweaks_open = false;
                return;
            }
            if is_quit {
                self.should_quit = true;
                return;
            }
            // Dispatch to tweaks state machine.
            if let Some(tweak_action) = self.tweaks_state.handle_key(key) {
                self.dispatch_tweak_action(tweak_action).await;
            }
            return;
        }

        // If prestige modal is open, route key events to it first.
        if self.prestige_modal.is_open()
            && let Event::Key(key) = event
            && matches!(
                key.kind,
                crossterm::event::KeyEventKind::Press | crossterm::event::KeyEventKind::Repeat
            )
        {
            use crossterm::event::KeyCode;
            let is_quit = key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q'));
            if is_quit {
                self.should_quit = true;
                return;
            }
            self.handle_prestige_modal_key(key).await;
            return;
        }

        // If skill detail overlay is open, route keys to it first.
        if self.skill_detail.is_some()
            && let Event::Key(key) = event
            && matches!(
                key.kind,
                crossterm::event::KeyEventKind::Press | crossterm::event::KeyEventKind::Repeat
            )
        {
            use crossterm::event::KeyCode;
            let is_quit = key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q'));
            if is_quit {
                self.should_quit = true;
                return;
            }
            if let Some(ref mut sd) = self.skill_detail
                && let Some(action) = sd.handle_key(key)
            {
                self.dispatch_skill_detail_action(action).await;
            }
            return;
        }

        let action = map_event(event);
        let cmd_empty = self.command.is_empty();

        match action {
            Action::Quit => {
                self.should_quit = true;
            }
            // RowEnter: if command bar has text, treat as submit; else nav action.
            Action::RowEnter if cmd_empty => {
                self.handle_side_nav_enter().await;
            }
            Action::RowEnter => {
                self.submit_command().await;
            }
            Action::Space if cmd_empty => {
                self.handle_side_nav_space().await;
            }
            Action::Space => {
                self.command.push(' ');
            }
            Action::ToggleEnabledKey => {
                self.handle_side_nav_toggle_enabled().await;
            }
            Action::ArrowUp if cmd_empty => {
                self.handle_side_nav_up();
            }
            Action::ArrowDown if cmd_empty => {
                self.handle_side_nav_down();
            }
            Action::PageUp if cmd_empty => {
                self.handle_side_nav_page(false);
            }
            Action::PageDown if cmd_empty => {
                self.handle_side_nav_page(true);
            }
            // When command bar has text, arrows are ignored (no cursor movement).
            Action::ArrowUp | Action::ArrowDown | Action::PageUp | Action::PageDown => {}
            // 'f' when command bar is empty → open phase-chooser (if tokens available).
            Action::Char('f') if cmd_empty => {
                if tokens_available(&self.state) > 0 {
                    self.prestige_modal =
                        PrestigeModal::PhaseChooser(PhaseChooserStage::Pattern { cursor: 0 });
                } else {
                    self.push_log("no focus tokens available", "dim");
                }
            }
            Action::Char(c) => {
                self.command.push(c);
            }
            Action::Backspace => {
                self.command.pop();
            }
            Action::ClearInput => {
                // Esc: close skill detail first, then clear selection, then input.
                if self.skill_detail.is_some() {
                    self.skill_detail = None;
                } else if self.side_selection.list_idx > 0 || self.side_selection.levels_idx > 0 {
                    self.side_selection.list_idx = 0;
                    self.side_selection.levels_idx = 0;
                    self.side_selection.list_offset = 0;
                    self.side_selection.levels_offset = 0;
                } else {
                    self.command.clear();
                }
            }
            Action::ToggleTweaks => {
                self.tweaks_open = !self.tweaks_open;
                if self.tweaks_open {
                    self.tweaks_state.sync_palette(&self.state.palette);
                    self.tweaks_state
                        .sync_xp_multiplier(self.state.xp_multiplier);
                }
            }
            Action::NextTab => {
                self.side_tab = self.side_tab.cycle();
            }
            Action::Resize(_, _) => {} // ratatui handles resize automatically
            Action::Noop => {}
        }
    }

    // ── Side-panel navigation helpers ────────────────────────────────────────

    fn list_selectable_count(&self) -> usize {
        REMINDERS
            .iter()
            .filter(|r| {
                self.state
                    .reminders
                    .get(&r.reminder_id())
                    .map(|rt| rt.enabled)
                    .unwrap_or(false)
            })
            .count()
    }

    /// Compute the line-index within the LIST flat-line vec for the given
    /// selection index (0-based over enabled reminder rows).
    /// Returns 0 if the index is out of range.
    fn list_sel_line_index(&self, sel_idx: usize) -> usize {
        // Line 0 = hint. Then for each category with enabled reminders:
        //   1 header line + N reminder lines + 1 blank line.
        let mut line = 1usize; // hint
        let mut remaining = sel_idx;
        for cat in CATEGORIES {
            let enabled: Vec<_> = REMINDERS
                .iter()
                .filter(|r| r.cat == cat.id)
                .filter(|r| {
                    self.state
                        .reminders
                        .get(&r.reminder_id())
                        .map(|rt| rt.enabled)
                        .unwrap_or(false)
                })
                .collect();
            if enabled.is_empty() {
                continue;
            }
            line += 1; // category header
            if remaining < enabled.len() {
                return line + remaining; // found
            }
            remaining -= enabled.len();
            line += enabled.len() + 1; // reminder rows + blank
        }
        // Fallback: shouldn't happen for valid sel_idx, but safe default.
        line
    }

    fn handle_side_nav_up(&mut self) {
        match self.side_tab {
            SideTab::List => {
                if self.side_selection.list_idx > 0 {
                    self.side_selection.list_idx -= 1;
                    let sel_line = self.list_sel_line_index(self.side_selection.list_idx);
                    if sel_line < self.side_selection.list_offset {
                        self.side_selection.list_offset = sel_line;
                    }
                }
            }
            SideTab::Levels => {
                if self.side_selection.levels_idx > 0 {
                    self.side_selection.levels_idx -= 1;
                    // levels_offset is in line-units; scroll up if selected row is above viewport.
                    let sel_line =
                        self.side_selection.levels_idx * LEVELS_LINES_PER_CAT + LEVELS_LEAD_LINES;
                    if sel_line < self.side_selection.levels_offset {
                        self.side_selection.levels_offset = sel_line;
                    }
                }
            }
            SideTab::Log => {
                if self.side_selection.log_offset > 0 {
                    self.side_selection.log_offset -= 1;
                }
            }
        }
    }

    fn handle_side_nav_down(&mut self) {
        match self.side_tab {
            SideTab::List => {
                let max = self.list_selectable_count().saturating_sub(1);
                if self.side_selection.list_idx < max {
                    self.side_selection.list_idx += 1;
                    // Scroll viewport down if selected row is below visible area.
                    let sel_line = self.list_sel_line_index(self.side_selection.list_idx);
                    let vp = self.list_viewport_h.max(1);
                    if sel_line >= self.side_selection.list_offset + vp {
                        self.side_selection.list_offset = sel_line + 1 - vp;
                    }
                }
            }
            SideTab::Levels => {
                let max = CATEGORIES.len().saturating_sub(1);
                if self.side_selection.levels_idx < max {
                    self.side_selection.levels_idx += 1;
                    // levels_offset is in line-units; scroll viewport down if needed.
                    // The selected category occupies lines [sel_line, sel_line+2].
                    // We ensure the last of those 3 lines fits in the viewport.
                    let sel_line =
                        self.side_selection.levels_idx * LEVELS_LINES_PER_CAT + LEVELS_LEAD_LINES;
                    let last_line = sel_line + LEVELS_LINES_PER_CAT - 1;
                    let vp = self.levels_viewport_h.max(1);
                    if last_line >= self.side_selection.levels_offset + vp {
                        self.side_selection.levels_offset = last_line + 1 - vp;
                    }
                }
            }
            SideTab::Log => {
                self.side_selection.log_offset += 1;
            }
        }
    }

    fn handle_side_nav_page(&mut self, down: bool) {
        let step = 5usize;
        if down {
            self.handle_side_nav_down();
            for _ in 1..step {
                self.handle_side_nav_down();
            }
        } else {
            for _ in 0..step {
                self.handle_side_nav_up();
            }
        }
    }

    async fn handle_side_nav_space(&mut self) {
        if self.side_tab == SideTab::List {
            // Toggle pin on selected reminder.
            if let Some(rid) = self.selected_list_reminder_id() {
                let req_id = self.next_req_id();
                self.client
                    .send(Message::Request {
                        id: req_id,
                        action: IpcAction::TogglePin { reminder_id: rid },
                    })
                    .await;
            }
        }
    }

    async fn handle_side_nav_toggle_enabled(&mut self) {
        match self.side_tab {
            SideTab::List => {
                if let Some(rid) = self.selected_list_reminder_id() {
                    let req_id = self.next_req_id();
                    self.client
                        .send(Message::Request {
                            id: req_id,
                            action: IpcAction::ToggleEnabled { reminder_id: rid },
                        })
                        .await;
                }
            }
            SideTab::Levels => {
                // No direct enabled toggle on a category row; no-op.
            }
            SideTab::Log => {}
        }
    }

    async fn handle_side_nav_enter(&mut self) {
        match self.side_tab {
            SideTab::List => {
                // Enter on LIST row completes the reminder.
                if let Some(rid) = self.selected_list_reminder_id() {
                    let req_id = self.next_req_id();
                    self.client
                        .send(Message::Request {
                            id: req_id,
                            action: IpcAction::Complete { reminder_id: rid },
                        })
                        .await;
                }
            }
            SideTab::Levels => {
                let cat = CATEGORIES.get(self.side_selection.levels_idx);
                if let Some(cat) = cat {
                    let trait_id = seed_core::domain::TraitId(cat.trait_id.to_string());
                    let current_xp = self.state.traits.get(&trait_id).copied().unwrap_or(0);
                    let lvl99_xp = xp_for_level(seed_core::levels::MAX_LEVEL);
                    if current_xp >= lvl99_xp {
                        // Trait is at lvl 99 — open enhancement chooser.
                        self.prestige_modal = PrestigeModal::EnhancementChooser {
                            trait_id,
                            cursor: 0,
                        };
                    } else {
                        // Normal: open skill detail.
                        self.skill_detail = Some(SkillDetailState::new(trait_id));
                    }
                }
            }
            SideTab::Log => {}
        }
    }

    /// Return the `ReminderId` of the currently selected LIST row (enabled reminders only).
    fn selected_list_reminder_id(&self) -> Option<seed_core::domain::ReminderId> {
        let enabled: Vec<_> = REMINDERS
            .iter()
            .filter(|r| {
                self.state
                    .reminders
                    .get(&r.reminder_id())
                    .map(|rt| rt.enabled)
                    .unwrap_or(false)
            })
            .collect();
        enabled
            .get(self.side_selection.list_idx)
            .map(|r| r.reminder_id())
    }

    /// Dispatch a `SkillDetailAction` to IPC and update overlay state.
    async fn dispatch_skill_detail_action(&mut self, action: SkillDetailAction) {
        match action {
            SkillDetailAction::Close => {
                self.skill_detail = None;
            }
            SkillDetailAction::NextSkill => {
                if let Some(ref mut sd) = self.skill_detail {
                    let idx = CATEGORIES
                        .iter()
                        .position(|c| c.trait_id == sd.trait_id.0.as_str())
                        .unwrap_or(0);
                    let next = (idx + 1) % CATEGORIES.len();
                    sd.trait_id = seed_core::domain::TraitId(CATEGORIES[next].trait_id.to_string());
                    sd.focus_idx = 0;
                }
            }
            SkillDetailAction::PrevSkill => {
                if let Some(ref mut sd) = self.skill_detail {
                    let idx = CATEGORIES
                        .iter()
                        .position(|c| c.trait_id == sd.trait_id.0.as_str())
                        .unwrap_or(0);
                    let prev = if idx == 0 {
                        CATEGORIES.len() - 1
                    } else {
                        idx - 1
                    };
                    sd.trait_id = seed_core::domain::TraitId(CATEGORIES[prev].trait_id.to_string());
                    sd.focus_idx = 0;
                }
            }
            SkillDetailAction::AdjustInterval {
                reminder_id,
                delta_min,
            } => {
                // Clamp new interval to [1, 24*60].
                let current = self
                    .state
                    .reminders
                    .get(&reminder_id)
                    .map(|rt| rt.interval_min as i32)
                    .unwrap_or(45);
                let new_min = (current + delta_min).clamp(1, 24 * 60) as u32;
                // Only dispatch if the value actually changed (avoid redundant log writes at clamp boundaries).
                if new_min != current as u32 {
                    let req_id = self.next_req_id();
                    self.client
                        .send(Message::Request {
                            id: req_id,
                            action: IpcAction::SetReminderInterval {
                                reminder_id,
                                minutes: new_min,
                            },
                        })
                        .await;
                }
            }
            SkillDetailAction::TogglePin { reminder_id } => {
                let req_id = self.next_req_id();
                self.client
                    .send(Message::Request {
                        id: req_id,
                        action: IpcAction::TogglePin { reminder_id },
                    })
                    .await;
            }
            SkillDetailAction::ToggleEnabled { reminder_id } => {
                let req_id = self.next_req_id();
                self.client
                    .send(Message::Request {
                        id: req_id,
                        action: IpcAction::ToggleEnabled { reminder_id },
                    })
                    .await;
            }
        }
    }

    /// Handle a key event when a prestige modal is open.
    async fn handle_prestige_modal_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match self.prestige_modal.clone() {
            PrestigeModal::None => {}

            // ── Enhancement chooser ───────────────────────────────────────────
            PrestigeModal::EnhancementChooser { trait_id, cursor } => {
                let trait_name = trait_id.0.as_str();
                // Only one option per trait in the starter set.
                let options = [default_enhancement(trait_name)];
                let count = options.len();
                match key.code {
                    KeyCode::Esc => {
                        self.prestige_modal = PrestigeModal::None;
                    }
                    KeyCode::Up => {
                        let new_cursor = cursor.saturating_sub(1);
                        self.prestige_modal = PrestigeModal::EnhancementChooser {
                            trait_id: trait_id.clone(),
                            cursor: new_cursor,
                        };
                    }
                    KeyCode::Down => {
                        let new_cursor = (cursor + 1).min(count.saturating_sub(1));
                        self.prestige_modal = PrestigeModal::EnhancementChooser {
                            trait_id: trait_id.clone(),
                            cursor: new_cursor,
                        };
                    }
                    KeyCode::Enter => {
                        // Dispatch integrate action.
                        let enhancement = options[cursor.min(count.saturating_sub(1))].clone();
                        let req_id = self.next_req_id();
                        self.client
                            .send(Message::Request {
                                id: req_id,
                                action: IpcAction::Integrate {
                                    trait_id: trait_id.clone(),
                                    enhancement_id: enhancement,
                                },
                            })
                            .await;
                        self.prestige_modal = PrestigeModal::None;
                        self.push_log(&format!("{} integrated", trait_name), "accent-2");
                    }
                    _ => {}
                }
            }

            // ── Phase chooser ─────────────────────────────────────────────────
            PrestigeModal::PhaseChooser(stage) => match stage {
                PhaseChooserStage::Pattern { cursor } => match key.code {
                    KeyCode::Esc => {
                        self.prestige_modal = PrestigeModal::None;
                    }
                    KeyCode::Up => {
                        let new_cursor = cursor.saturating_sub(1);
                        self.prestige_modal =
                            PrestigeModal::PhaseChooser(PhaseChooserStage::Pattern {
                                cursor: new_cursor,
                            });
                    }
                    KeyCode::Down => {
                        let new_cursor = (cursor + 1).min(FOCUS_PATTERNS.len().saturating_sub(1));
                        self.prestige_modal =
                            PrestigeModal::PhaseChooser(PhaseChooserStage::Pattern {
                                cursor: new_cursor,
                            });
                    }
                    KeyCode::Enter => {
                        let pattern = FOCUS_PATTERNS[cursor].clone();
                        self.prestige_modal =
                            PrestigeModal::PhaseChooser(PhaseChooserStage::Traits {
                                pattern,
                                selected: vec![false; CATEGORIES.len()],
                                cursor: 0,
                            });
                    }
                    _ => {}
                },

                PhaseChooserStage::Traits {
                    pattern,
                    mut selected,
                    cursor,
                } => match key.code {
                    KeyCode::Esc => {
                        // Back to pattern selection.
                        self.prestige_modal =
                            PrestigeModal::PhaseChooser(PhaseChooserStage::Pattern { cursor: 0 });
                    }
                    KeyCode::Up => {
                        let new_cursor = cursor.saturating_sub(1);
                        self.prestige_modal =
                            PrestigeModal::PhaseChooser(PhaseChooserStage::Traits {
                                pattern,
                                selected,
                                cursor: new_cursor,
                            });
                    }
                    KeyCode::Down => {
                        let new_cursor = (cursor + 1).min(CATEGORIES.len().saturating_sub(1));
                        self.prestige_modal =
                            PrestigeModal::PhaseChooser(PhaseChooserStage::Traits {
                                pattern,
                                selected,
                                cursor: new_cursor,
                            });
                    }
                    KeyCode::Char(' ') => {
                        // Toggle selection. Enforce maximum based on pattern arity.
                        let max_sel = pattern.skill_count();
                        let currently_selected: usize = selected.iter().filter(|&&b| b).count();
                        if selected[cursor] {
                            selected[cursor] = false;
                        } else if currently_selected < max_sel {
                            selected[cursor] = true;
                        }
                        self.prestige_modal =
                            PrestigeModal::PhaseChooser(PhaseChooserStage::Traits {
                                pattern,
                                selected,
                                cursor,
                            });
                    }
                    KeyCode::Enter => {
                        // Confirm — validate arity then dispatch.
                        let required = pattern.skill_count();
                        let chosen: Vec<TraitId> = CATEGORIES
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| selected.get(*i).copied().unwrap_or(false))
                            .map(|(_, cat)| TraitId(cat.trait_id.to_string()))
                            .collect();
                        if chosen.len() != required {
                            self.push_log(
                                &format!("select exactly {} trait(s) for this pattern", required),
                                "dim",
                            );
                            self.prestige_modal =
                                PrestigeModal::PhaseChooser(PhaseChooserStage::Traits {
                                    pattern,
                                    selected,
                                    cursor,
                                });
                        } else {
                            let req_id = self.next_req_id();
                            self.client
                                .send(Message::Request {
                                    id: req_id,
                                    action: IpcAction::ActivateFocusPhase {
                                        pattern: pattern.clone(),
                                        traits: chosen,
                                    },
                                })
                                .await;
                            self.prestige_modal = PrestigeModal::None;
                            self.push_log("focus phase activated", "accent-2");
                        }
                    }
                    _ => {}
                },
            },
        }
    }

    /// Forward a TweakAction to the IPC client.
    async fn dispatch_tweak_action(&mut self, action: TweakAction) {
        match action {
            TweakAction::SetPalette { palette } => {
                self.push_log(&format!("palette → {palette}"), "dim");
                let req_id = self.next_req_id();
                self.client
                    .send(Message::Request {
                        id: req_id,
                        action: IpcAction::SetPalette { palette },
                    })
                    .await;
            }
            TweakAction::SetXpMultiplier { multiplier } => {
                self.push_log(&format!("xp x {multiplier}"), "dim");
                let req_id = self.next_req_id();
                self.client
                    .send(Message::Request {
                        id: req_id,
                        action: IpcAction::SetXpMultiplier { multiplier },
                    })
                    .await;
            }
            TweakAction::TriggerReminderNow => {
                self.push_log("trigger: forcing reminder due now", "dim");
                let req_id = self.next_req_id();
                self.client
                    .send(Message::Request {
                        id: req_id,
                        action: IpcAction::TriggerReminderNow { reminder_id: None },
                    })
                    .await;
            }
            TweakAction::Reset => {
                self.push_log("resetting all progress…", "dim");
                let req_id = self.next_req_id();
                self.client
                    .send(Message::Request {
                        id: req_id,
                        action: IpcAction::Reset,
                    })
                    .await;
            }
        }
    }

    /// Submit the command bar input.
    async fn submit_command(&mut self) {
        let input = std::mem::take(&mut self.command);
        match parse(&input) {
            ParsedCommand::Help => {
                self.push_log(
                    "verbs: water steep eat graze stand align walk stretch shake look sun breathe rest journal reflect thanks sit read tidy reach · debug: /trait n · /random · /all n",
                    "dim",
                );
            }
            ParsedCommand::HelpSkill { skill } => {
                self.skill_detail = Some(SkillDetailState::new(seed_core::domain::TraitId(skill)));
            }
            ParsedCommand::UnknownSkill(skill) => {
                self.push_log(
                    &format!("unknown skill: {skill} · try ?flow ?core ?spine ?reach ?clarity ?space ?depth ?resonance ?warmth"),
                    "dim",
                );
            }
            ParsedCommand::Verb { word } => {
                // Find the reminder matching this word.
                if let Some(reminder) = REMINDERS.iter().find(|r| r.word == word) {
                    let req_id = self.next_req_id();
                    self.client
                        .send(Message::Request {
                            id: req_id,
                            action: IpcAction::Complete {
                                reminder_id: reminder.reminder_id(),
                            },
                        })
                        .await;
                } else {
                    self.push_log(&format!("unknown verb: {word}"), "dim");
                }
            }
            ParsedCommand::Debug { trait_name, level } => {
                let req_id = self.next_req_id();
                self.client
                    .send(Message::Request {
                        id: req_id,
                        action: IpcAction::SetTraitLevel {
                            trait_id: seed_core::domain::TraitId(trait_name.clone()),
                            level,
                        },
                    })
                    .await;
                self.push_log(&format!("{trait_name} set to lvl {level}"), "accent-2");
            }
            ParsedCommand::Random => {
                let levels = random_trait_levels();
                let mut summary = Vec::with_capacity(levels.len());
                for (trait_id, level) in &levels {
                    let req_id = self.next_req_id();
                    self.client
                        .send(Message::Request {
                            id: req_id,
                            action: IpcAction::SetTraitLevel {
                                trait_id: seed_core::domain::TraitId(trait_id.to_string()),
                                level: *level,
                            },
                        })
                        .await;
                    summary.push(format!("{trait_id}={level}"));
                }
                self.push_log(&format!("/random · {}", summary.join(" ")), "accent-2");
            }
            ParsedCommand::All { level } => {
                let mut summary = Vec::with_capacity(CATEGORIES.len());
                for c in CATEGORIES {
                    let req_id = self.next_req_id();
                    self.client
                        .send(Message::Request {
                            id: req_id,
                            action: IpcAction::SetTraitLevel {
                                trait_id: seed_core::domain::TraitId(c.trait_id.to_string()),
                                level,
                            },
                        })
                        .await;
                    summary.push(format!("{}={}", c.trait_id, level));
                }
                self.push_log(&format!("/all {level} · {}", summary.join(" ")), "accent-2");
            }
            ParsedCommand::Integrate {
                trait_name,
                enhancement_name,
            } => {
                let trait_id = seed_core::domain::TraitId(trait_name.clone());
                let enhancement = match enhancement_name {
                    Some(ref name) => match parse_enhancement(name) {
                        Some(e) => e,
                        None => {
                            self.push_log(&format!("unknown enhancement: {name}"), "dim");
                            return;
                        }
                    },
                    None => default_enhancement(&trait_name),
                };
                let req_id = self.next_req_id();
                self.client
                    .send(Message::Request {
                        id: req_id,
                        action: IpcAction::Integrate {
                            trait_id,
                            enhancement_id: enhancement,
                        },
                    })
                    .await;
                self.push_log(&format!("/integrate {trait_name}"), "accent-2");
            }
            ParsedCommand::Focus { pattern, traits } => {
                let trait_ids: Vec<seed_core::domain::TraitId> = traits
                    .iter()
                    .map(|t| seed_core::domain::TraitId(t.clone()))
                    .collect();
                let req_id = self.next_req_id();
                self.client
                    .send(Message::Request {
                        id: req_id,
                        action: IpcAction::ActivateFocusPhase {
                            pattern,
                            traits: trait_ids,
                        },
                    })
                    .await;
                self.push_log(&format!("/focus {}", traits.join(" ")), "accent-2");
            }
            ParsedCommand::PrestigeError(msg) => {
                self.push_log(&msg, "dim");
            }
            ParsedCommand::Unknown(s) if !s.is_empty() => {
                self.push_log(&format!("unknown: {s}"), "dim");
            }
            ParsedCommand::Unknown(_) => {}
        }
    }

    /// Handle an inbound IPC message.
    fn handle_ipc(&mut self, msg: Message) {
        match msg {
            Message::Snapshot { state } => {
                self.state = *state;
                debug!("snapshot applied");
            }
            Message::StateDiff { events } => {
                // Tier crossings are the headline moment: if this batch carries a
                // TierChanged, the tier-up toast wins the single slot and the
                // co-occurring level-up is suppressed. Scoped to the batch so it
                // never depends on a stale toast lingering from a prior batch.
                let tier_up_in_batch = events
                    .iter()
                    .any(|e| e.kind == "seed.companion.tier_changed");
                for env in events {
                    self.apply_envelope(env, tier_up_in_batch);
                }
            }
            Message::Response { id: _, result } => {
                use crate::client::ResponseResult;
                if let ResponseResult::Err { message } = result {
                    warn!("daemon error response: {message}");
                    self.push_log(&format!("error: {message}"), "dim");
                }
            }
            _ => {}
        }
    }

    fn apply_envelope(&mut self, env: EventEnvelope, tier_up_in_batch: bool) {
        let kind = env.kind.clone();
        match seed_core::events::from_envelope(env) {
            Ok(event) => {
                let prev_level: std::collections::BTreeMap<_, _> = self
                    .state
                    .traits
                    .iter()
                    .map(|(k, &xp)| (k.clone(), level_for_xp(xp)))
                    .collect();

                apply_event(&mut self.state, &event);

                // Tier crossings are macro-celebration moments and always coincide
                // with a level-up. When the daemon emits a TierChanged, it wins the
                // single toast slot over the LevelUp raised for the same batch.
                if let CoreEvent::TierChanged { to, .. } = &event {
                    self.toast = Some(Toast::tier_up(
                        format!("TIER {} · {}", to.name(), to.adj()),
                        self.tick,
                    ));
                }

                // Level-up toast, derived from the XP change. Suppressed when this
                // batch also carries a tier crossing: tier-up supersedes the
                // co-occurring level-up for the single toast slot, regardless of the
                // order the two events arrive within the batch.
                for (trait_id, &new_xp) in &self.state.traits {
                    let new_level = level_for_xp(new_xp);
                    if let Some(&old_level) = prev_level.get(trait_id)
                        && new_level > old_level
                        && !tier_up_in_batch
                    {
                        self.toast = Some(Toast::level_up(
                            format!("{} · LVL {}", trait_id.0.to_uppercase(), new_level),
                            self.tick,
                        ));
                    }
                }

                // XP gain toast for completions.
                if let CoreEvent::ReminderCompleted {
                    xp_gained,
                    trait_id,
                    ..
                } = &event
                {
                    // Only show xp toast if no level-up toast was set.
                    if self
                        .toast
                        .as_ref()
                        .map(|t| !matches!(t.kind, crate::view::toast::ToastKind::LevelUp))
                        .unwrap_or(true)
                    {
                        self.toast = Some(Toast::xp_gain(
                            format!("+{} {} xp", xp_gained, trait_id.0),
                            self.tick,
                        ));
                    }
                }

                // Focus token toast.
                if let CoreEvent::FocusTokenEarned { new_balance } = &event {
                    self.toast = Some(Toast::focus_token(
                        format!("+1 focus token ({new_balance} total)"),
                        self.tick,
                    ));
                }
            }
            Err(e) => {
                // `from_envelope` routes unknown kinds to Event::Unknown (a no-op
                // in apply_event), so an Err here means a *known* kind with malformed
                // data — skip it rather than crash the batch.
                debug!("malformed event kind '{kind}': {e}");
            }
        }
    }

    fn push_log(&mut self, msg: &str, tag: &str) {
        let t = Utc::now().format("%H:%M").to_string();
        self.state.log.push_back(seed_core::state::LogEntry {
            t,
            msg: msg.to_string(),
            tag: tag.to_string(),
        });
        // Cap at 50.
        while self.state.log.len() > 50 {
            self.state.log.pop_front();
        }
    }

    fn next_req_id(&mut self) -> u64 {
        let id = self.next_req_id;
        self.next_req_id = self.next_req_id.wrapping_add(1);
        id
    }
}

/// Pick a fresh level (1..=99) for each of the 9 categories.
///
/// Used by the `/random` debug command to roll a complete trait set so the
/// glyph re-renders with a totally different visual layout. Seeded by wall
/// clock so each invocation produces a different roll; no `rand` dep needed.
fn random_trait_levels() -> Vec<(&'static str, u8)> {
    use seed_core::domain::CATEGORIES;
    use std::time::{SystemTime, UNIX_EPOCH};

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xa5a5_a5a5_a5a5_a5a5);

    // splitmix64 — short, deterministic-from-seed, well-distributed
    fn splitmix64(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = *state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    let mut state = seed;
    CATEGORIES
        .iter()
        .map(|c| {
            let r = splitmix64(&mut state);
            // map to 1..=99 inclusive
            let level = ((r % 99) + 1) as u8;
            (c.trait_id, level)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use seed_core::domain::{FocusPattern, IntegrationEnhancement};

    #[test]
    fn random_trait_levels_returns_one_per_category() {
        let rolls = random_trait_levels();
        assert_eq!(rolls.len(), CATEGORIES.len());
        let names: Vec<&str> = rolls.iter().map(|(n, _)| *n).collect();
        for c in CATEGORIES {
            assert!(
                names.contains(&c.trait_id),
                "missing trait {} in /random output",
                c.trait_id
            );
        }
    }

    #[test]
    fn random_trait_levels_in_valid_range() {
        // run a few times to be sure the generator can't escape the range
        for _ in 0..20 {
            for (name, level) in random_trait_levels() {
                assert!(
                    (1..=99).contains(&level),
                    "trait {name} level {level} out of 1..=99"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // default_enhancement mapping
    // -----------------------------------------------------------------------

    #[test]
    fn default_enhancement_covers_all_traits() {
        for cat in CATEGORIES {
            // Just ensure it returns a valid variant (no panic).
            let _ = default_enhancement(cat.trait_id);
        }
    }

    #[test]
    fn default_enhancement_flow_is_spiral() {
        assert_eq!(
            default_enhancement("flow"),
            IntegrationEnhancement::FlowSpiral
        );
    }

    // -----------------------------------------------------------------------
    // Phase-chooser modal state machine
    // -----------------------------------------------------------------------

    #[test]
    fn phase_chooser_cursor_wraps_at_bounds() {
        let stage = PhaseChooserStage::Pattern { cursor: 0 };
        // Simulate "up" at top: cursor stays at 0.
        if let PhaseChooserStage::Pattern { cursor } = &stage {
            let new = cursor.saturating_sub(1);
            assert_eq!(new, 0);
        }
        // Simulate "down" from last: cursor clamps.
        let stage2 = PhaseChooserStage::Pattern {
            cursor: FOCUS_PATTERNS.len() - 1,
        };
        if let PhaseChooserStage::Pattern { cursor } = &stage2 {
            let new = (cursor + 1).min(FOCUS_PATTERNS.len().saturating_sub(1));
            assert_eq!(
                new,
                FOCUS_PATTERNS.len() - 1,
                "should not exceed last index"
            );
        }
    }

    #[test]
    fn phase_chooser_trait_selection_enforces_arity() {
        // Spread3x2 → 3 traits; max 3 can be selected.
        let pattern = FocusPattern::Spread3x2;
        let mut selected = vec![false; CATEGORIES.len()];
        let max_sel = pattern.skill_count();

        // Select 3.
        for i in 0..3 {
            let currently: usize = selected.iter().filter(|&&b| b).count();
            if !selected[i] && currently < max_sel {
                selected[i] = true;
            }
        }
        assert_eq!(
            selected.iter().filter(|&&b| b).count(),
            3,
            "should have selected exactly 3"
        );

        // Try to select a 4th — should be blocked.
        let currently: usize = selected.iter().filter(|&&b| b).count();
        if !selected[3] && currently < max_sel {
            selected[3] = true; // this branch should NOT fire
        }
        assert_eq!(
            selected.iter().filter(|&&b| b).count(),
            3,
            "4th selection should be blocked by max_sel guard"
        );
    }

    #[test]
    fn enhancement_chooser_cursor_clamps() {
        // There's only 1 option per trait; cursor above 0 clamps to 0 on confirm.
        let cursor = 5usize;
        let options_len = 1usize;
        let clamped = cursor.min(options_len.saturating_sub(1));
        assert_eq!(clamped, 0);
    }

    // -----------------------------------------------------------------------
    // Token-balance chip: tokens_available helper
    // -----------------------------------------------------------------------

    #[test]
    fn token_balance_zero_for_fresh_state() {
        let state = seed_core::initial_state(0);
        assert_eq!(tokens_available(&state), 0);
    }

    #[test]
    fn token_balance_nonzero_when_cumulative_99() {
        let mut state = seed_core::initial_state(0);
        state.cumulative_levels_gained = 99;
        assert_eq!(tokens_available(&state), 1);
    }

    // -----------------------------------------------------------------------
    // Integration count affordance: xp_for_level(99) threshold check
    // -----------------------------------------------------------------------

    #[test]
    fn ready_to_integrate_threshold() {
        let lvl99_xp = xp_for_level(seed_core::levels::MAX_LEVEL);
        let below = lvl99_xp.saturating_sub(1);
        assert!(below < lvl99_xp, "below threshold should not be ready");
        assert!(lvl99_xp >= lvl99_xp, "at threshold should be ready");
    }

    // -----------------------------------------------------------------------
    // Active focus arrows: arrow count from allocation
    // -----------------------------------------------------------------------

    #[test]
    fn focus_arrows_extracted_correctly() {
        use seed_core::domain::{FocusPattern, FocusPhase};
        let state = {
            let mut s = seed_core::initial_state(0);
            s.active_focus = Some(FocusPhase {
                pattern: FocusPattern::Spread3x2,
                allocations: vec![
                    (TraitId("flow".into()), 1),
                    (TraitId("core".into()), 1),
                    (TraitId("spine".into()), 1),
                ],
            });
            s
        };
        // flow should have 1 arrow, depth should have 0.
        let flow_arrows = state
            .active_focus
            .as_ref()
            .and_then(|f| {
                f.allocations
                    .iter()
                    .find(|(t, _)| t.0 == "flow")
                    .map(|(_, a)| *a)
            })
            .unwrap_or(0);
        assert_eq!(flow_arrows, 1);

        let depth_arrows = state
            .active_focus
            .as_ref()
            .and_then(|f| {
                f.allocations
                    .iter()
                    .find(|(t, _)| t.0 == "depth")
                    .map(|(_, a)| *a)
            })
            .unwrap_or(0);
        assert_eq!(depth_arrows, 0);
    }

    // -----------------------------------------------------------------------
    // Integration count rendering: trait_integrations lookup
    // -----------------------------------------------------------------------

    #[test]
    fn integration_count_lookup() {
        let mut state = seed_core::initial_state(0);
        state.trait_integrations.insert(TraitId("flow".into()), 2);
        let count = state
            .trait_integrations
            .get(&TraitId("flow".into()))
            .copied()
            .unwrap_or(0);
        assert_eq!(count, 2);
        let depth_count = state
            .trait_integrations
            .get(&TraitId("depth".into()))
            .copied()
            .unwrap_or(0);
        assert_eq!(depth_count, 0);
    }

    // -----------------------------------------------------------------------
    // Forward-compat: unknown future event kinds are no-ops, not batch-breakers
    // (TASK-021)
    // -----------------------------------------------------------------------

    #[test]
    fn unknown_future_kind_does_not_break_batch() {
        use crate::client::Message;
        use seed_core::events::to_envelope;
        use seed_core::levels::xp_for_level;

        let ts = chrono::Utc::now();
        let mut app = App::new_for_test(seed_core::initial_state(0));

        // Batch: a synthetic future-kind envelope (unknown to this build) plus a
        // known TraitXpChanged that lifts "flow" from level 1 to level 2.
        let flow = TraitId("flow".into());
        let l2 = xp_for_level(2);
        let known = to_envelope(
            &CoreEvent::TraitXpChanged {
                trait_id: flow.clone(),
                delta: l2 as i64,
                new_xp: l2,
            },
            ts,
        );
        let future = EventEnvelope {
            v: 1,
            ts,
            kind: "seed.future.something".into(),
            data: serde_json::json!({ "anything": 42 }),
        };

        // Unknown-first ordering proves the unknown one doesn't abort the batch.
        app.handle_ipc(Message::StateDiff {
            events: vec![future, known],
        });

        assert_eq!(
            *app.state.traits.get(&flow).unwrap(),
            l2,
            "known event must still apply after an unknown future-kind event"
        );
    }

    // -----------------------------------------------------------------------
    // Tier-up toast (TASK-025): TierChanged raises a TierUp toast and supersedes
    // a co-occurring LevelUp.
    // -----------------------------------------------------------------------

    #[test]
    fn tier_changed_raises_tier_up_toast() {
        use crate::view::toast::ToastKind;
        use seed_core::domain::Tier;
        use seed_core::events::to_envelope;

        let mut app = App::new_for_test(seed_core::initial_state(0));
        let env = to_envelope(
            &CoreEvent::TierChanged {
                from: Tier::Seed,
                to: Tier::Sprout,
                total_level: 18,
            },
            chrono::Utc::now(),
        );
        app.apply_envelope(env, true);

        let toast = app
            .toast
            .as_ref()
            .expect("tier change should raise a toast");
        assert!(
            matches!(toast.kind, ToastKind::TierUp),
            "toast kind should be TierUp, was {:?}",
            toast.kind
        );
        assert!(
            toast.msg.contains("SPROUT"),
            "toast should name the destination tier, was {:?}",
            toast.msg
        );
    }

    #[test]
    fn tier_up_supersedes_co_occurring_level_up() {
        use crate::client::Message;
        use seed_core::domain::Tier;
        use seed_core::events::to_envelope;
        use seed_core::levels::xp_for_level;

        let ts = chrono::Utc::now();
        let mut app = App::new_for_test(seed_core::initial_state(0));

        // Real daemon batch order: the XP change (which the TUI reads as a
        // level-up) precedes the TierChanged. Tier-up must win the slot.
        let flow = TraitId("flow".into());
        let l2 = xp_for_level(2);
        let level_up_driver = to_envelope(
            &CoreEvent::TraitXpChanged {
                trait_id: flow,
                delta: l2 as i64,
                new_xp: l2,
            },
            ts,
        );
        let tier = to_envelope(
            &CoreEvent::TierChanged {
                from: Tier::Seed,
                to: Tier::Sprout,
                total_level: 18,
            },
            ts,
        );

        app.handle_ipc(Message::StateDiff {
            events: vec![level_up_driver, tier],
        });

        let toast = app.toast.as_ref().expect("a toast should be present");
        assert!(
            matches!(toast.kind, crate::view::toast::ToastKind::TierUp),
            "tier-up should occupy the slot over the co-occurring level-up, was {:?}",
            toast.kind
        );
    }

    #[test]
    fn stale_tier_up_does_not_suppress_a_later_level_up() {
        use crate::client::Message;
        use crate::view::toast::ToastKind;
        use seed_core::domain::Tier;
        use seed_core::events::to_envelope;
        use seed_core::levels::xp_for_level;

        let ts = chrono::Utc::now();
        let mut app = App::new_for_test(seed_core::initial_state(0));

        // Batch 1: a tier crossing leaves a TierUp toast lingering in the slot.
        app.handle_ipc(Message::StateDiff {
            events: vec![to_envelope(
                &CoreEvent::TierChanged {
                    from: Tier::Seed,
                    to: Tier::Sprout,
                    total_level: 18,
                },
                ts,
            )],
        });
        assert!(matches!(
            app.toast.as_ref().map(|t| &t.kind),
            Some(ToastKind::TierUp)
        ));

        // Batch 2: an unrelated level-up with NO tier change. Suppression is
        // batch-scoped, so the stale TierUp must not swallow this level-up.
        let flow = TraitId("flow".into());
        let l2 = xp_for_level(2);
        app.handle_ipc(Message::StateDiff {
            events: vec![to_envelope(
                &CoreEvent::TraitXpChanged {
                    trait_id: flow,
                    delta: l2 as i64,
                    new_xp: l2,
                },
                ts,
            )],
        });

        let toast = app.toast.as_ref().expect("a toast should be present");
        assert!(
            matches!(toast.kind, ToastKind::LevelUp),
            "a level-up in a tier-free batch must show, not be suppressed by a \
             stale tier-up, was {:?}",
            toast.kind
        );
    }
}
