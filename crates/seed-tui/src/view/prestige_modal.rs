/// Prestige modal overlays: EnhancementChooser and PhaseChooser.
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};
use seed_core::{
    domain::{CATEGORIES, FocusPattern, IntegrationEnhancement, TraitId},
    state::State,
};

use crate::{
    palette::Palette,
    prestige::{FOCUS_PATTERNS, PhaseChooserStage, default_enhancement},
    view::skill_detail::centered_rect,
};

// ---------------------------------------------------------------------------
// Enhancement chooser
// ---------------------------------------------------------------------------

pub struct EnhancementChooserWidget<'a> {
    pub trait_id: &'a TraitId,
    pub cursor: usize,
    pub app_state: &'a State,
    pub palette: &'a Palette,
    pub truecolor: bool,
}

/// Human-readable name for an IntegrationEnhancement.
fn enhancement_name(e: &IntegrationEnhancement) -> &'static str {
    match e {
        IntegrationEnhancement::FlowSpiral => "Flow Spiral",
        IntegrationEnhancement::CoreEmber => "Core Ember",
        IntegrationEnhancement::SpineLattice => "Spine Lattice",
        IntegrationEnhancement::ReachBranch => "Reach Branch",
        IntegrationEnhancement::ClarityRing => "Clarity Ring",
        IntegrationEnhancement::SpaceVeil => "Space Veil",
        IntegrationEnhancement::DepthAbyss => "Depth Abyss",
        IntegrationEnhancement::ResonanceChord => "Resonance Chord",
        IntegrationEnhancement::WarmthGlow => "Warmth Glow",
    }
}

impl Widget for EnhancementChooserWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use crate::palette::downgrade_color;

        let accent = downgrade_color(self.palette.accent, self.truecolor);
        let fg = downgrade_color(self.palette.fg_bright, self.truecolor);
        let fg_dim = downgrade_color(self.palette.fg, self.truecolor);
        let warm = downgrade_color(self.palette.warm, self.truecolor);
        let bg2 = downgrade_color(self.palette.bg2, self.truecolor);
        let bg_select = downgrade_color(self.palette.border_bright, self.truecolor);

        let panel = centered_rect(area, 50, 50, 44, 12);
        Clear.render(panel, buf);

        let trait_name = self.trait_id.0.as_str();
        let integrations = self
            .app_state
            .trait_integrations
            .get(self.trait_id)
            .copied()
            .unwrap_or(0);

        // Build enhancement options list (one starter per trait for now).
        let options = [default_enhancement(trait_name)];

        let mut lines: Vec<Line> = vec![
            Line::from(Span::styled(
                format!("INTEGRATE — {}", trait_name.to_uppercase()),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                if integrations > 0 {
                    format!("  Prior integrations: {integrations}")
                } else {
                    "  First integration".to_string()
                },
                Style::default().fg(fg_dim),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "  Choose enhancement:",
                Style::default().fg(fg),
            )),
        ];

        for (i, enhancement) in options.iter().enumerate() {
            let selected = i == self.cursor.min(options.len().saturating_sub(1));
            let row_style = if selected {
                Style::default()
                    .fg(warm)
                    .bg(bg_select)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(fg_dim)
            };
            let marker = if selected { ">" } else { " " };
            lines.push(Line::from(Span::styled(
                format!("  {marker} {}", enhancement_name(enhancement)),
                row_style,
            )));
        }

        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            "Enter confirm  Esc cancel",
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
// Phase chooser
// ---------------------------------------------------------------------------

pub struct PhaseChooserWidget<'a> {
    pub stage: &'a PhaseChooserStage,
    pub palette: &'a Palette,
    pub truecolor: bool,
}

fn pattern_display(p: &FocusPattern) -> &'static str {
    match p {
        FocusPattern::Spread3x2 => "Spread 3x2  — 3 traits  ▲ each (2×)",
        FocusPattern::Spread2x3 => "Spread 2x3  — 2 traits  ▲▲ each (3×)",
        FocusPattern::Concentrate1x4 => "Concentrate — 1 trait   ▲▲▲ (4×)",
    }
}

impl Widget for PhaseChooserWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use crate::palette::downgrade_color;

        let accent = downgrade_color(self.palette.accent, self.truecolor);
        let fg = downgrade_color(self.palette.fg_bright, self.truecolor);
        let fg_dim = downgrade_color(self.palette.fg, self.truecolor);
        let warm = downgrade_color(self.palette.warm, self.truecolor);
        let bg2 = downgrade_color(self.palette.bg2, self.truecolor);
        let bg_select = downgrade_color(self.palette.border_bright, self.truecolor);

        let panel = centered_rect(area, 55, 65, 46, 18);
        Clear.render(panel, buf);

        let mut lines: Vec<Line> = vec![Line::from(Span::styled(
            "ACTIVATE FOCUS PHASE",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ))];

        match self.stage {
            PhaseChooserStage::Pattern { cursor } => {
                lines.push(Line::from(Span::styled(
                    "  Select allocation pattern:",
                    Style::default().fg(fg),
                )));
                lines.push(Line::from(Span::raw("")));

                for (i, pattern) in FOCUS_PATTERNS.iter().enumerate() {
                    let selected = i == *cursor;
                    let row_style = if selected {
                        Style::default()
                            .fg(warm)
                            .bg(bg_select)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(fg_dim)
                    };
                    let marker = if selected { ">" } else { " " };
                    lines.push(Line::from(Span::styled(
                        format!("  {marker} {}", pattern_display(pattern)),
                        row_style,
                    )));
                }

                lines.push(Line::from(Span::raw("")));
                lines.push(Line::from(Span::styled(
                    "↑↓ select  Enter confirm  Esc cancel",
                    Style::default().fg(fg_dim),
                )));
            }

            PhaseChooserStage::Traits {
                pattern,
                selected,
                cursor,
            } => {
                let required = pattern.skill_count();
                let chosen_count = selected.iter().filter(|&&b| b).count();
                lines.push(Line::from(Span::styled(
                    format!(
                        "  {} — select {} trait(s)  ({}/{} chosen)",
                        match pattern {
                            FocusPattern::Spread3x2 => "Spread 3x2",
                            FocusPattern::Spread2x3 => "Spread 2x3",
                            FocusPattern::Concentrate1x4 => "Concentrate",
                        },
                        required,
                        chosen_count,
                        required,
                    ),
                    Style::default().fg(fg),
                )));
                lines.push(Line::from(Span::raw("")));

                for (i, cat) in CATEGORIES.iter().enumerate() {
                    let is_cursor = i == *cursor;
                    let is_selected = selected.get(i).copied().unwrap_or(false);
                    let check = if is_selected { "[x]" } else { "[ ]" };
                    let row_style = if is_cursor {
                        Style::default()
                            .fg(warm)
                            .bg(bg_select)
                            .add_modifier(Modifier::BOLD)
                    } else if is_selected {
                        Style::default().fg(accent)
                    } else {
                        Style::default().fg(fg_dim)
                    };
                    lines.push(Line::from(Span::styled(
                        format!("  {} {} {}", check, cat.icon, cat.trait_id),
                        row_style,
                    )));
                }

                lines.push(Line::from(Span::raw("")));
                lines.push(Line::from(Span::styled(
                    "↑↓ move  Space toggle  Enter confirm  Esc back",
                    Style::default().fg(fg_dim),
                )));
            }
        }

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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enhancement_names_non_empty() {
        let enhancements = [
            IntegrationEnhancement::FlowSpiral,
            IntegrationEnhancement::CoreEmber,
            IntegrationEnhancement::SpineLattice,
            IntegrationEnhancement::ReachBranch,
            IntegrationEnhancement::ClarityRing,
            IntegrationEnhancement::SpaceVeil,
            IntegrationEnhancement::DepthAbyss,
            IntegrationEnhancement::ResonanceChord,
            IntegrationEnhancement::WarmthGlow,
        ];
        for e in &enhancements {
            assert!(!enhancement_name(e).is_empty());
        }
    }

    #[test]
    fn pattern_display_non_empty() {
        for p in crate::prestige::FOCUS_PATTERNS {
            assert!(!pattern_display(p).is_empty());
        }
    }
}
