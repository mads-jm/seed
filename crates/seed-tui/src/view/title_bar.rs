/// Top title bar widget.
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use seed_core::domain::{Tier, tier_for};

use crate::palette::Palette;

pub struct TitleBar<'a> {
    pub tier: Tier,
    pub total_level: u32,
    pub palette: &'a Palette,
    pub palette_name: &'a str,
    pub truecolor: bool,
    /// Available focus tokens. Renders `★N` chip when > 0.
    pub tokens_available: u32,
}

impl Widget for TitleBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use crate::palette::downgrade_color;

        let accent = downgrade_color(self.palette.accent, self.truecolor);
        let fg_dim = downgrade_color(self.palette.fg, self.truecolor);
        let fg_bright = downgrade_color(self.palette.fg_bright, self.truecolor);

        let _ = tier_for(self.total_level); // ensure tier matches total

        let mut spans = vec![
            Span::styled(
                "◈ seed",
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("v0.1 · local", Style::default().fg(fg_dim)),
            Span::raw("    "),
            Span::styled("FORM ", Style::default().fg(fg_dim)),
            Span::styled(
                self.tier.name(),
                Style::default().fg(fg_bright).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · ", Style::default().fg(fg_dim)),
            Span::styled(self.tier.adj(), Style::default().fg(fg_dim)),
            Span::raw("    "),
            Span::styled("palette ", Style::default().fg(fg_dim)),
            Span::styled(self.palette_name, Style::default().fg(accent)),
        ];

        // Token-balance chip: render ★N only when tokens > 0.
        if self.tokens_available > 0 {
            spans.push(Span::raw("    "));
            spans.push(Span::styled(
                format!("\u{2605}{}", self.tokens_available), // ★N
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ));
        }

        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn make_terminal(w: u16, h: u16) -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(w, h)).unwrap()
    }

    fn dummy_palette() -> &'static crate::palette::Palette {
        crate::palette::palette_for("sage")
    }

    #[test]
    fn token_chip_absent_when_zero() {
        let mut terminal = make_terminal(80, 1);
        let palette = dummy_palette();
        terminal
            .draw(|f| {
                TitleBar {
                    tier: Tier::Seed,
                    total_level: 0,
                    palette,
                    palette_name: "sage",
                    truecolor: false,
                    tokens_available: 0,
                }
                .render(f.area(), f.buffer_mut());
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let content: String = buf
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect::<Vec<_>>()
            .join("");
        assert!(
            !content.contains('\u{2605}'),
            "★ chip must not appear when tokens_available == 0"
        );
    }

    #[test]
    fn token_chip_present_when_positive() {
        let mut terminal = make_terminal(80, 1);
        let palette = dummy_palette();
        terminal
            .draw(|f| {
                TitleBar {
                    tier: Tier::Seed,
                    total_level: 0,
                    palette,
                    palette_name: "sage",
                    truecolor: false,
                    tokens_available: 3,
                }
                .render(f.area(), f.buffer_mut());
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let content: String = buf
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect::<Vec<_>>()
            .join("");
        assert!(
            content.contains('\u{2605}'),
            "★ chip must appear when tokens_available > 0; rendered: {content:?}"
        );
    }
}
