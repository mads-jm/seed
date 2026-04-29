/// Bottom command input bar.
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Widget,
};

use crate::palette::Palette;

pub struct CommandBar<'a> {
    pub input: &'a str,
    pub palette: &'a Palette,
    pub truecolor: bool,
}

impl Widget for CommandBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use crate::palette::downgrade_color;

        let accent = downgrade_color(self.palette.accent, self.truecolor);
        let fg_dim = downgrade_color(self.palette.fg, self.truecolor);
        let fg = downgrade_color(self.palette.fg_bright, self.truecolor);

        let placeholder = "action · water · breathe · look · help · /flow 50";

        let (input_span, color) = if self.input.is_empty() {
            (
                Span::styled(placeholder, Style::default().fg(fg_dim)),
                fg_dim,
            )
        } else {
            (Span::styled(self.input, Style::default().fg(fg)), fg)
        };

        let _ = color;

        let line = Line::from(vec![
            Span::styled("› ", Style::default().fg(accent)),
            input_span,
        ]);

        buf.set_line(area.x, area.y, &line, area.width);
    }
}
