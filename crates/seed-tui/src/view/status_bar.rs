/// Bottom-bottom status bar showing key hints and wellness status.
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

use crate::palette::Palette;

pub struct StatusBar<'a> {
    pub completed_total: u32,
    pub any_overdue: bool,
    pub wellness: f32,
    pub palette: &'a Palette,
    pub truecolor: bool,
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use crate::palette::downgrade_color;

        let fg_dim = downgrade_color(self.palette.fg, self.truecolor);
        let accent = downgrade_color(self.palette.accent, self.truecolor);
        let overdue_color = downgrade_color(self.palette.overdue, self.truecolor);

        let status_text = if self.any_overdue {
            Span::styled(
                "COMPANION WILTING",
                Style::default()
                    .fg(overdue_color)
                    .add_modifier(Modifier::BOLD),
            )
        } else if self.wellness > 0.75 {
            Span::styled(
                "IN BLOOM",
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled("STEADY", Style::default().fg(fg_dim))
        };

        let hint_style = Style::default().fg(fg_dim);

        let completed_str = format!("COMPLETED {}", self.completed_total);

        let line = Line::from(vec![
            Span::styled("[ / ]", hint_style),
            Span::styled(" focus  ", hint_style),
            Span::styled("[ CTRL+T ]", hint_style),
            Span::styled(" tweaks  ", hint_style),
            Span::styled("[ ENTER ]", hint_style),
            Span::styled(" commit  ", hint_style),
            Span::styled("[ Q ]", hint_style),
            Span::styled(" quit", hint_style),
            Span::raw("    "),
            Span::styled(completed_str, hint_style),
            Span::styled("  ·  ", hint_style),
            status_text,
        ]);

        buf.set_line(area.x, area.y, &line, area.width);
    }
}
