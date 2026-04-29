/// Transient toast overlay shown after completion or level-up.
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

use crate::palette::Palette;

#[derive(Debug, Clone)]
pub enum ToastKind {
    XpGain,
    LevelUp,
    FocusToken,
    #[allow(dead_code)]
    Other,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub msg: String,
    pub kind: ToastKind,
    /// Tick at which the toast was created (for 2.2s display window).
    pub created_tick: u32,
}

impl Toast {
    pub fn xp_gain(msg: impl Into<String>, tick: u32) -> Self {
        Toast {
            msg: msg.into(),
            kind: ToastKind::XpGain,
            created_tick: tick,
        }
    }

    pub fn level_up(msg: impl Into<String>, tick: u32) -> Self {
        Toast {
            msg: msg.into(),
            kind: ToastKind::LevelUp,
            created_tick: tick,
        }
    }

    pub fn focus_token(msg: impl Into<String>, tick: u32) -> Self {
        Toast {
            msg: msg.into(),
            kind: ToastKind::FocusToken,
            created_tick: tick,
        }
    }

    /// Returns `true` if the toast should still be visible at `current_tick`.
    /// At ~20 Hz, 2.2s ≈ 44 ticks.
    pub fn is_visible(&self, current_tick: u32) -> bool {
        current_tick.saturating_sub(self.created_tick) < 44
    }
}

pub struct ToastWidget<'a> {
    pub toast: &'a Toast,
    pub palette: &'a Palette,
    pub truecolor: bool,
}

impl Widget for ToastWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use crate::palette::downgrade_color;

        let color = match self.toast.kind {
            ToastKind::XpGain => downgrade_color(self.palette.accent, self.truecolor),
            ToastKind::LevelUp => downgrade_color(self.palette.accent2, self.truecolor),
            ToastKind::FocusToken => downgrade_color(self.palette.warm, self.truecolor),
            ToastKind::Other => downgrade_color(self.palette.warm, self.truecolor),
        };

        // Place toast near top-center.
        let width = (self.toast.msg.len() as u16 + 4).min(area.width);
        let x = area.x + area.width.saturating_sub(width) / 2;
        let toast_area = Rect {
            x,
            y: area.y + 1,
            width,
            height: 3,
        };

        // Clear background cells.
        Clear.render(toast_area, buf);

        let para = Paragraph::new(Line::from(vec![Span::styled(
            format!("  {}  ", self.toast.msg),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(color)),
        )
        .alignment(Alignment::Center);

        para.render(toast_area, buf);
    }
}
