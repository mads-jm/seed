/// Skill detail overlay — shows per-skill info, reminders, and interval controls.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};
use seed_core::{
    domain::{
        CATEGORIES, REMINDERS, ReminderId, TraitId, reminder_status, reminder_status_with_interval,
    },
    glyph::trait_color,
    levels::{level_for_xp, level_progress, xp_to_next},
    state::State,
};

use crate::palette::Palette;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Persistent state for the skill detail overlay.
pub struct SkillDetailState {
    /// Which trait is being shown.
    pub trait_id: TraitId,
    /// Which reminder row (within this skill's reminders) is focused.
    pub focus_idx: usize,
}

impl SkillDetailState {
    pub fn new(trait_id: TraitId) -> Self {
        SkillDetailState {
            trait_id,
            focus_idx: 0,
        }
    }

    /// Handle a key while the overlay is open. Returns Some(action) to dispatch.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<SkillDetailAction> {
        // Shift+Tab → prev skill.
        if key.code == KeyCode::BackTab
            || (key.modifiers.contains(KeyModifiers::SHIFT) && key.code == KeyCode::Tab)
        {
            return Some(SkillDetailAction::PrevSkill);
        }

        // Tab → next skill.
        if key.code == KeyCode::Tab {
            return Some(SkillDetailAction::NextSkill);
        }

        let reminders = skill_reminders(&self.trait_id);
        let rem_count = reminders.len();

        match key.code {
            KeyCode::Esc => return Some(SkillDetailAction::Close),

            KeyCode::Up if self.focus_idx > 0 => {
                self.focus_idx -= 1;
            }
            KeyCode::Down if rem_count > 0 && self.focus_idx + 1 < rem_count => {
                self.focus_idx += 1;
            }

            KeyCode::Left | KeyCode::Char('<') => {
                if let Some(rid) = reminders.get(self.focus_idx).map(|r| r.reminder_id()) {
                    let delta = if key.modifiers.contains(KeyModifiers::SHIFT) {
                        -15
                    } else {
                        -5
                    };
                    return Some(SkillDetailAction::AdjustInterval {
                        reminder_id: rid,
                        delta_min: delta,
                    });
                }
            }
            KeyCode::Right | KeyCode::Char('>') => {
                if let Some(rid) = reminders.get(self.focus_idx).map(|r| r.reminder_id()) {
                    let delta = if key.modifiers.contains(KeyModifiers::SHIFT) {
                        15
                    } else {
                        5
                    };
                    return Some(SkillDetailAction::AdjustInterval {
                        reminder_id: rid,
                        delta_min: delta,
                    });
                }
            }

            KeyCode::Char(' ') => {
                if let Some(rid) = reminders.get(self.focus_idx).map(|r| r.reminder_id()) {
                    return Some(SkillDetailAction::TogglePin { reminder_id: rid });
                }
            }
            KeyCode::Char('e') => {
                if let Some(rid) = reminders.get(self.focus_idx).map(|r| r.reminder_id()) {
                    return Some(SkillDetailAction::ToggleEnabled { reminder_id: rid });
                }
            }

            _ => {}
        }
        None
    }
}

/// Actions the skill detail can emit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillDetailAction {
    AdjustInterval {
        reminder_id: ReminderId,
        delta_min: i32,
    },
    TogglePin {
        reminder_id: ReminderId,
    },
    ToggleEnabled {
        reminder_id: ReminderId,
    },
    Close,
    NextSkill,
    PrevSkill,
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

pub struct SkillDetail<'a> {
    pub state: &'a SkillDetailState,
    pub app_state: &'a State,
    pub palette: &'a Palette,
    pub truecolor: bool,
}

impl Widget for SkillDetail<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use crate::palette::downgrade_color;
        use chrono::Utc;

        let accent = downgrade_color(self.palette.accent, self.truecolor);
        let fg = downgrade_color(self.palette.fg_bright, self.truecolor);
        let fg_dim = downgrade_color(self.palette.fg, self.truecolor);
        let bg2 = downgrade_color(self.palette.bg2, self.truecolor);
        let bg_select = downgrade_color(self.palette.border_bright, self.truecolor);

        // Centered overlay: 60% width × 70% height, min 50×16.
        let panel = centered_rect(area, 60, 70, 50, 16);
        Clear.render(panel, buf);

        let trait_id_str = self.state.trait_id.0.as_str();

        // Look up the category for this trait.
        let cat = CATEGORIES.iter().find(|c| c.trait_id == trait_id_str);
        let (cat_name, cat_icon, cat_desc) = cat
            .map(|c| (c.name, c.icon, c.description))
            .unwrap_or(("UNKNOWN", "?", ""));

        let xp = self
            .app_state
            .traits
            .get(&self.state.trait_id)
            .copied()
            .unwrap_or(0);
        let lvl = level_for_xp(xp);
        let prog = level_progress(xp);
        let to_next = xp_to_next(xp);

        // Trait color for the bar.
        let bar_color = if self.truecolor {
            trait_color(trait_id_str, prog)
        } else {
            downgrade_color(trait_color(trait_id_str, prog), false)
        };

        let bar_w = (panel.width as usize).saturating_sub(4).max(8);
        let filled = (prog * bar_w as f32).round() as usize;
        let bar: String = (0..bar_w)
            .map(|i| if i < filled { '█' } else { '░' })
            .collect();

        let reminders = skill_reminders(&self.state.trait_id);
        let now_ms = Utc::now().timestamp_millis();

        let mut lines: Vec<Line> = vec![
            // Header
            Line::from(vec![Span::styled(
                format!(
                    "{cat_icon} {cat_name}  lvl {lvl}  ·  {to_next} to {}",
                    lvl + 1
                ),
                Style::default().fg(fg).add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(bar, Style::default().fg(bar_color)),
            ]),
            Line::from(Span::styled(
                format!("  {xp} xp"),
                Style::default().fg(fg_dim),
            )),
            Line::from(Span::raw("")),
            // Description
            Line::from(Span::styled(
                format!("  {cat_desc}"),
                Style::default().fg(fg_dim),
            )),
            Line::from(Span::raw("")),
            // Reminders header
            Line::from(Span::styled(
                "  REMINDERS",
                Style::default().fg(fg).add_modifier(Modifier::BOLD),
            )),
        ];

        for (i, r) in reminders.iter().enumerate() {
            let rt = self.app_state.reminders.get(&r.reminder_id());
            let interval = rt.map(|rt| rt.interval_min).unwrap_or(r.interval_min);
            let pinned = rt.map(|rt| rt.pinned).unwrap_or(false);
            let enabled = rt.map(|rt| rt.enabled).unwrap_or(true);
            let status = rt
                .map(|rt| {
                    reminder_status_with_interval(
                        rt.interval_min,
                        rt.last_done_ms,
                        rt.enabled,
                        now_ms,
                    )
                })
                .unwrap_or_else(|| reminder_status(r, 0, false, now_ms));

            let pin_str = if pinned { "★" } else { " " };
            let en_str = if enabled { "on" } else { "off" };
            let state_str = match status.state {
                seed_core::domain::ReminderState::Due => "DUE",
                seed_core::domain::ReminderState::Overdue => "OVRD",
                seed_core::domain::ReminderState::Dormant => "...",
                seed_core::domain::ReminderState::Off => "OFF",
            };

            let selected = i == self.state.focus_idx;
            let row_style = if selected {
                Style::default().bg(bg_select).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            lines.push(Line::from(vec![Span::styled(
                format!(
                    "  {pin_str} {:<12} {:>4}m  [{en_str}]  {state_str}",
                    r.name, interval,
                ),
                row_style.fg(if selected { fg } else { fg_dim }),
            )]));
        }

        // Integration count (B5): show when > 0.
        let integrations = self
            .app_state
            .trait_integrations
            .get(&self.state.trait_id)
            .copied()
            .unwrap_or(0);
        if integrations > 0 {
            lines.push(Line::from(Span::styled(
                format!("  Integrations: {integrations}"),
                Style::default()
                    .fg(downgrade_color(self.palette.warm, self.truecolor))
                    .add_modifier(Modifier::BOLD),
            )));
        }

        // Per-trait skip surface: show only when there is something to surface.
        if let Some(stats) = self.app_state.traits_skipped.get(&self.state.trait_id)
            && stats.lifetime > 0
        {
            let count_7d = stats.count_7d(now_ms);
            let skip_color = if count_7d >= 3 {
                downgrade_color(self.palette.due, self.truecolor)
            } else {
                fg_dim
            };
            let skip_line = if count_7d > 0 {
                format!(
                    "  Skipped: {count_7d} (7d) \u{00b7} {} lifetime",
                    stats.lifetime
                )
            } else {
                format!("  Skipped: {} lifetime", stats.lifetime)
            };
            lines.push(Line::from(Span::styled(
                skip_line,
                Style::default().fg(skip_color),
            )));
        }

        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            "Esc close · ↑↓ select · ←/→ adj · Shift±15m · Space pin · e enable · Tab next",
            Style::default().fg(fg_dim),
        )));

        let para = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(accent))
                    .style(Style::default().bg(bg2)),
            )
            .alignment(Alignment::Left);
        para.render(panel, buf);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return all static reminders bound to the given trait (via category).
pub fn skill_reminders(trait_id: &TraitId) -> Vec<&'static seed_core::domain::Reminder> {
    let cat_id = CATEGORIES
        .iter()
        .find(|c| c.trait_id == trait_id.0.as_str())
        .map(|c| c.id);
    let Some(cat_id) = cat_id else {
        return Vec::new();
    };
    REMINDERS.iter().filter(|r| r.cat == cat_id).collect()
}

/// Compute a centered Rect within `area`.
/// `pct_w` and `pct_h` are percentages (0-100). `min_w` and `min_h` are pixel floors.
pub fn centered_rect(area: Rect, pct_w: u16, pct_h: u16, min_w: u16, min_h: u16) -> Rect {
    let w = ((area.width as u32 * pct_w as u32) / 100)
        .max(min_w as u32)
        .min(area.width as u32) as u16;
    let h = ((area.height as u32 * pct_h as u32) / 100)
        .max(min_h as u32)
        .min(area.height as u32) as u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}
