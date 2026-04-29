/// Floating tweaks panel (Ctrl+T to toggle).
///
/// The panel renders palette swatches and action buttons. Event handling is
/// done by `TweaksPanelState::handle_key`, which returns an optional
/// `TweakAction` that `app.rs` forwards to the IPC client.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

use crate::palette::Palette;

/// Actions the tweaks panel can emit. App forwards these to the IPC client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TweakAction {
    SetPalette { palette: String },
    SetXpMultiplier { multiplier: u32 },
    TriggerReminderNow,
    Reset,
}

/// Focused item within the tweaks panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TweaksFocus {
    #[default]
    Palette,
    XpMultiplier,
    Trigger,
    Reset,
}

/// Persistent state for the tweaks panel (focused item, confirm state).
#[derive(Debug, Default)]
pub struct TweaksPanelState {
    pub focus: TweaksFocus,
    /// Which palette swatch is focused (0..5).
    pub palette_idx: usize,
    /// Which XP multiplier is focused (index into XP_MULTIPLIERS).
    pub xp_multiplier_idx: usize,
    /// True when the RESET confirm prompt is active.
    pub confirm_reset: bool,
}

pub const PALETTES: &[&str] = &["sage", "dusk", "mist", "ember", "moss"];
pub const XP_MULTIPLIERS: &[u32] = &[1, 2, 5, 10, 50, 100];

impl TweaksPanelState {
    /// Handle a key event while the tweaks panel is open.
    /// Returns `Some(TweakAction)` if an IPC action should be dispatched.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<TweakAction> {
        // Esc cancels confirm-reset.
        if self.confirm_reset {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.confirm_reset = false;
                    return Some(TweakAction::Reset);
                }
                _ => {
                    self.confirm_reset = false;
                    return None;
                }
            }
        }

        match key.code {
            // Tab / Shift-Tab cycles between rows.
            KeyCode::Tab => {
                self.focus = match self.focus {
                    TweaksFocus::Palette => TweaksFocus::XpMultiplier,
                    TweaksFocus::XpMultiplier => TweaksFocus::Trigger,
                    TweaksFocus::Trigger => TweaksFocus::Reset,
                    TweaksFocus::Reset => TweaksFocus::Palette,
                };
            }
            KeyCode::BackTab => {
                self.focus = match self.focus {
                    TweaksFocus::Palette => TweaksFocus::Reset,
                    TweaksFocus::XpMultiplier => TweaksFocus::Palette,
                    TweaksFocus::Trigger => TweaksFocus::XpMultiplier,
                    TweaksFocus::Reset => TweaksFocus::Trigger,
                };
            }
            // Left / Right navigate the focused row.
            KeyCode::Left => match self.focus {
                TweaksFocus::Palette => {
                    if self.palette_idx > 0 {
                        self.palette_idx -= 1;
                    }
                }
                TweaksFocus::XpMultiplier => {
                    if self.xp_multiplier_idx > 0 {
                        self.xp_multiplier_idx -= 1;
                    }
                }
                _ => {}
            },
            KeyCode::Right => match self.focus {
                TweaksFocus::Palette => {
                    if self.palette_idx + 1 < PALETTES.len() {
                        self.palette_idx += 1;
                    }
                }
                TweaksFocus::XpMultiplier => {
                    if self.xp_multiplier_idx + 1 < XP_MULTIPLIERS.len() {
                        self.xp_multiplier_idx += 1;
                    }
                }
                _ => {}
            },
            // Enter / Space activates the focused item.
            KeyCode::Enter | KeyCode::Char(' ') => {
                return self.activate(key.modifiers);
            }
            _ => {}
        }
        None
    }

    fn activate(&mut self, _modifiers: KeyModifiers) -> Option<TweakAction> {
        match self.focus {
            TweaksFocus::Palette => {
                let name = PALETTES[self.palette_idx.min(PALETTES.len() - 1)];
                Some(TweakAction::SetPalette {
                    palette: name.to_string(),
                })
            }
            TweaksFocus::XpMultiplier => {
                let multiplier =
                    XP_MULTIPLIERS[self.xp_multiplier_idx.min(XP_MULTIPLIERS.len() - 1)];
                Some(TweakAction::SetXpMultiplier { multiplier })
            }
            TweaksFocus::Trigger => Some(TweakAction::TriggerReminderNow),
            TweaksFocus::Reset => {
                // First press: open confirm prompt; action fires on second confirmation.
                self.confirm_reset = true;
                None
            }
        }
    }

    /// Sync palette_idx to match the current active palette name (call on open).
    pub fn sync_palette(&mut self, current: &str) {
        if let Some(i) = PALETTES.iter().position(|&p| p == current) {
            self.palette_idx = i;
        }
    }

    /// Sync xp_multiplier_idx to match the current multiplier value (call on open).
    /// Falls back to index 0 (x1) if the exact value is not in XP_MULTIPLIERS.
    pub fn sync_xp_multiplier(&mut self, current: u32) {
        self.xp_multiplier_idx = XP_MULTIPLIERS
            .iter()
            .position(|&m| m == current)
            .unwrap_or(0);
    }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

pub struct TweaksPanel<'a> {
    pub state: &'a TweaksPanelState,
    pub palette_name: &'a str,
    pub palette: &'a Palette,
    pub truecolor: bool,
    /// Current active XP multiplier (from app state, used for [active] marker).
    pub xp_multiplier: u32,
}

impl Widget for TweaksPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use crate::palette::downgrade_color;

        let accent = downgrade_color(self.palette.accent, self.truecolor);
        let fg = downgrade_color(self.palette.fg_bright, self.truecolor);
        let fg_dim = downgrade_color(self.palette.fg, self.truecolor);
        let overdue = downgrade_color(self.palette.overdue, self.truecolor);
        let focused_style = Style::default().fg(accent).add_modifier(Modifier::BOLD);

        // Float panel: bottom-right corner, 38 wide x 13 tall.
        let w = 38u16.min(area.width);
        let h = 13u16.min(area.height);
        let panel = Rect {
            x: area.x + area.width.saturating_sub(w + 2),
            y: area.y + area.height.saturating_sub(h + 3),
            width: w,
            height: h,
        };

        Clear.render(panel, buf);

        // Build palette swatch row.
        let palette_row: String = PALETTES
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let is_active = *p == self.palette_name;
                let is_focused =
                    self.state.focus == TweaksFocus::Palette && i == self.state.palette_idx;
                if is_focused {
                    format!(">{p}< ")
                } else if is_active {
                    format!("[{p}] ")
                } else {
                    format!(" {p}  ")
                }
            })
            .collect();

        // Build XP multiplier row.
        let xp_row: String = XP_MULTIPLIERS
            .iter()
            .enumerate()
            .map(|(i, &m)| {
                let is_active = m == self.xp_multiplier;
                let is_focused = self.state.focus == TweaksFocus::XpMultiplier
                    && i == self.state.xp_multiplier_idx;
                let label = format!("×{m}");
                if is_focused {
                    format!(">{label}< ")
                } else if is_active {
                    format!("[{label}] ")
                } else {
                    format!(" {label}  ")
                }
            })
            .collect();

        let trigger_style = if self.state.focus == TweaksFocus::Trigger {
            focused_style
        } else {
            Style::default().fg(fg_dim)
        };
        let reset_style = if self.state.focus == TweaksFocus::Reset {
            focused_style.fg(overdue)
        } else {
            Style::default().fg(overdue)
        };

        let confirm_line = if self.state.confirm_reset {
            Line::from(Span::styled(
                "  Confirm RESET? y=yes  any=cancel",
                Style::default().fg(overdue).add_modifier(Modifier::BOLD),
            ))
        } else {
            Line::from(Span::raw(""))
        };

        let lines = vec![
            Line::from(Span::styled(
                "TWEAKS",
                Style::default().fg(fg).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "─────────────────────────────────────",
                Style::default().fg(fg_dim),
            )),
            Line::from(vec![
                Span::styled("PALETTE  ", Style::default().fg(fg_dim)),
                Span::styled(
                    palette_row.trim_end().to_string(),
                    Style::default().fg(accent),
                ),
            ]),
            Line::from(vec![
                Span::styled("XP ×     ", Style::default().fg(fg_dim)),
                Span::styled(xp_row.trim_end().to_string(), Style::default().fg(accent)),
            ]),
            Line::from(Span::styled(
                "  Tab/Shift-Tab focus · L/R select · Enter activate",
                Style::default().fg(fg_dim),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "[ TRIGGER REMINDER ]  force due now",
                trigger_style,
            )),
            Line::from(Span::styled("[ RESET ]  wipes all progress", reset_style)),
            confirm_line,
            Line::from(Span::raw("")),
            Line::from(Span::styled("CTRL+T to close", Style::default().fg(fg_dim))),
        ];

        let para = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(fg_dim)),
            )
            .alignment(Alignment::Left);

        para.render(panel, buf);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tweaks_palette_activation_emits_set_palette() {
        let mut state = TweaksPanelState {
            focus: TweaksFocus::Palette,
            palette_idx: 2, // "mist"
            ..Default::default()
        };

        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let action = state.handle_key(key);

        assert_eq!(
            action,
            Some(TweakAction::SetPalette {
                palette: "mist".to_string()
            })
        );
    }

    #[test]
    fn tweaks_trigger_activation_emits_trigger() {
        let mut state = TweaksPanelState {
            focus: TweaksFocus::Trigger,
            ..Default::default()
        };

        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let action = state.handle_key(key);

        assert_eq!(action, Some(TweakAction::TriggerReminderNow));
    }

    #[test]
    fn tweaks_reset_requires_confirm() {
        let mut state = TweaksPanelState {
            focus: TweaksFocus::Reset,
            ..Default::default()
        };

        // First Enter: opens confirm prompt, no action yet.
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let action = state.handle_key(key);
        assert_eq!(action, None);
        assert!(state.confirm_reset);

        // Second Enter ('y'): confirms and emits Reset.
        let key2 = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
        let action2 = state.handle_key(key2);
        assert_eq!(action2, Some(TweakAction::Reset));
        assert!(!state.confirm_reset);
    }

    #[test]
    fn tweaks_reset_cancel_clears_confirm() {
        let mut state = TweaksPanelState {
            focus: TweaksFocus::Reset,
            ..Default::default()
        };

        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        state.handle_key(key);
        assert!(state.confirm_reset);

        // Escape cancels.
        let key2 = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let action = state.handle_key(key2);
        assert_eq!(action, None);
        assert!(!state.confirm_reset);
    }

    #[test]
    fn tweaks_tab_cycles_focus() {
        let mut state = TweaksPanelState::default();
        assert_eq!(state.focus, TweaksFocus::Palette);

        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        state.handle_key(tab);
        assert_eq!(state.focus, TweaksFocus::XpMultiplier);

        state.handle_key(tab);
        assert_eq!(state.focus, TweaksFocus::Trigger);

        state.handle_key(tab);
        assert_eq!(state.focus, TweaksFocus::Reset);

        state.handle_key(tab);
        assert_eq!(state.focus, TweaksFocus::Palette);
    }

    #[test]
    fn tweaks_palette_left_right_navigation() {
        let mut state = TweaksPanelState {
            focus: TweaksFocus::Palette,
            palette_idx: 0,
            ..Default::default()
        };

        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        state.handle_key(right);
        assert_eq!(state.palette_idx, 1);

        let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        state.handle_key(left);
        assert_eq!(state.palette_idx, 0);

        // Can't go below 0.
        state.handle_key(left);
        assert_eq!(state.palette_idx, 0);
    }

    #[test]
    fn tweaks_xp_multiplier_activation_emits_set_xp_multiplier() {
        let mut state = TweaksPanelState {
            focus: TweaksFocus::XpMultiplier,
            xp_multiplier_idx: 3, // x10
            ..Default::default()
        };

        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let action = state.handle_key(key);

        assert_eq!(
            action,
            Some(TweakAction::SetXpMultiplier { multiplier: 10 })
        );
    }

    #[test]
    fn tweaks_xp_multiplier_left_right_navigation() {
        let mut state = TweaksPanelState {
            focus: TweaksFocus::XpMultiplier,
            xp_multiplier_idx: 0,
            ..Default::default()
        };

        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        state.handle_key(right);
        assert_eq!(state.xp_multiplier_idx, 1);

        let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        state.handle_key(left);
        assert_eq!(state.xp_multiplier_idx, 0);

        // Can't go below 0.
        state.handle_key(left);
        assert_eq!(state.xp_multiplier_idx, 0);

        // Can't exceed last index.
        for _ in 0..100 {
            state.handle_key(right);
        }
        assert_eq!(state.xp_multiplier_idx, XP_MULTIPLIERS.len() - 1);
    }

    #[test]
    fn tweaks_sync_xp_multiplier_known_value() {
        let mut state = TweaksPanelState::default();
        state.sync_xp_multiplier(10);
        assert_eq!(state.xp_multiplier_idx, 3); // x10 is index 3
    }

    #[test]
    fn tweaks_sync_xp_multiplier_unknown_falls_back_to_zero() {
        let mut state = TweaksPanelState::default();
        state.sync_xp_multiplier(7); // not in the list
        assert_eq!(state.xp_multiplier_idx, 0);
    }
}
