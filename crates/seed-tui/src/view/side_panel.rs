/// Right side panel with LIST / LEVELS / LOG tabs.
use chrono::Utc;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
        Tabs, Widget,
    },
};
use seed_core::{
    domain::{CATEGORIES, REMINDERS, ReminderState, reminder_status_with_interval},
    glyph::trait_color,
    levels::{MAX_LEVEL, level_for_xp, level_progress, xp_for_level, xp_to_next},
    state::State,
};

use crate::palette::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SideTab {
    #[default]
    List,
    Levels,
    Log,
}

impl SideTab {
    pub fn cycle(self) -> Self {
        match self {
            SideTab::List => SideTab::Levels,
            SideTab::Levels => SideTab::Log,
            SideTab::Log => SideTab::List,
        }
    }

    pub fn index(self) -> usize {
        match self {
            SideTab::List => 0,
            SideTab::Levels => 1,
            SideTab::Log => 2,
        }
    }
}

pub struct SidePanel<'a> {
    pub state: &'a State,
    pub tab: SideTab,
    pub palette: &'a Palette,
    pub truecolor: bool,
    /// Selected row index in LIST (0-based over enabled reminders).
    pub list_idx: usize,
    pub list_offset: usize,
    /// Selected row index in LEVELS (0-based, by category).
    pub levels_idx: usize,
    pub levels_offset: usize,
    /// Scroll offset for LOG.
    pub log_offset: usize,
    /// Written-back rendered viewport height for LIST (lines). Used by nav handler next frame.
    pub list_viewport_h: &'a mut usize,
    /// Written-back rendered viewport height for LEVELS (lines). Used by nav handler next frame.
    pub levels_viewport_h: &'a mut usize,
}

impl Widget for SidePanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use crate::palette::downgrade_color;

        let accent = downgrade_color(self.palette.accent, self.truecolor);
        let fg_dim = downgrade_color(self.palette.fg, self.truecolor);
        let bg2 = downgrade_color(self.palette.bg2, self.truecolor);

        // Outer block.
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg_dim))
            .style(Style::default().bg(bg2));

        let inner = block.inner(area);
        block.render(area, buf);

        // Split: tabs row (1 line) + content.
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(0)])
            .split(inner);

        // Tab bar.
        let tab_titles = vec!["LIST", "LEVELS", "LOG"];
        let tabs = Tabs::new(tab_titles)
            .select(self.tab.index())
            .style(Style::default().fg(fg_dim))
            .highlight_style(
                Style::default()
                    .fg(accent)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
            .divider("|");
        tabs.render(chunks[0], buf);

        let content_area = chunks[1];
        let now_ms = Utc::now().timestamp_millis();

        match self.tab {
            SideTab::List => {
                let h = render_list(
                    self.state,
                    content_area,
                    buf,
                    self.palette,
                    self.truecolor,
                    now_ms,
                    self.list_idx,
                    self.list_offset,
                );
                *self.list_viewport_h = h;
            }
            SideTab::Levels => {
                let h = render_levels(
                    self.state,
                    content_area,
                    buf,
                    self.palette,
                    self.truecolor,
                    self.levels_idx,
                    self.levels_offset,
                );
                *self.levels_viewport_h = h;
            }
            SideTab::Log => render_log(
                self.state,
                content_area,
                buf,
                self.palette,
                self.truecolor,
                self.log_offset,
            ),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_list(
    state: &State,
    area: Rect,
    buf: &mut Buffer,
    palette: &Palette,
    truecolor: bool,
    now_ms: i64,
    list_idx: usize,
    list_offset: usize,
) -> usize {
    use crate::palette::downgrade_color;

    let fg_dim = downgrade_color(palette.fg, truecolor);
    let fg_bright = downgrade_color(palette.fg_bright, truecolor);
    let accent = downgrade_color(palette.accent, truecolor);
    let due_color = downgrade_color(palette.due, truecolor);
    let overdue_color = downgrade_color(palette.overdue, truecolor);
    // Selection highlight: slightly brightened bg.
    let bg_select = downgrade_color(palette.border_bright, truecolor);

    let hint = Line::from(Span::styled(
        "★=pinned  ↑↓ sel  Space pin  e enable",
        Style::default().fg(fg_dim),
    ));

    // Build all content lines, tracking which are selectable reminder rows.
    let mut all_lines: Vec<(Line, bool)> = vec![(hint, false)]; // (line, is_selectable)
    let mut selectable_row_line: Vec<usize> = Vec::new(); // line index for each selectable row

    for cat in CATEGORIES {
        let cat_reminders: Vec<_> = REMINDERS
            .iter()
            .filter(|r| r.cat == cat.id)
            .filter(|r| {
                state
                    .reminders
                    .get(&r.reminder_id())
                    .map(|rt| rt.enabled)
                    .unwrap_or(false)
            })
            .collect();

        if cat_reminders.is_empty() {
            continue;
        }

        let trait_xp = state
            .traits
            .get(&seed_core::domain::TraitId(cat.trait_id.to_string()))
            .copied()
            .unwrap_or(0);
        let trait_lvl = level_for_xp(trait_xp);

        all_lines.push((
            Line::from(vec![
                Span::styled(
                    format!("{} {} ", cat.icon, cat.name),
                    Style::default().fg(fg_dim),
                ),
                Span::styled(format!("lvl {trait_lvl}"), Style::default().fg(accent)),
            ]),
            false,
        ));

        for r in &cat_reminders {
            let rt = match state.reminders.get(&r.reminder_id()) {
                Some(rt) => rt,
                None => continue,
            };
            let status =
                reminder_status_with_interval(rt.interval_min, rt.last_done_ms, rt.enabled, now_ms);
            let pin_mark = if rt.pinned { "★" } else { " " };

            let (state_str, state_color) = match status.state {
                ReminderState::Due => ("DUE", due_color),
                ReminderState::Overdue => ("OVRD", overdue_color),
                ReminderState::Dormant => ("...", fg_dim),
                ReminderState::Off => ("OFF", fg_dim),
            };

            let time_str = match status.state {
                ReminderState::Dormant => {
                    let secs = status.ms_left / 1000;
                    let m = secs / 60;
                    let s = secs % 60;
                    format!("{m:02}:{s:02}")
                }
                _ => state_str.to_string(),
            };

            let bar_width = 8usize;
            let filled = (status.pct * bar_width as f32).round() as usize;
            let bar: String = (0..bar_width)
                .map(|i| if i < filled { '█' } else { '░' })
                .collect();

            selectable_row_line.push(all_lines.len());
            all_lines.push((
                Line::from(vec![
                    Span::styled(format!("{pin_mark} "), Style::default().fg(fg_dim)),
                    Span::styled(format!("{:<12}", r.name), Style::default().fg(fg_bright)),
                    Span::styled(
                        format!("{:>5} ", time_str),
                        Style::default().fg(state_color),
                    ),
                    Span::styled(bar, Style::default().fg(accent)),
                    Span::styled(format!(" {}", r.word), Style::default().fg(fg_dim)),
                ]),
                true,
            ));
        }

        all_lines.push((Line::from(""), false));
    }

    // Apply selection highlight.
    if let Some(&sel_line) = selectable_row_line.get(list_idx)
        && let Some((line, _)) = all_lines.get_mut(sel_line)
    {
        *line = line.clone().style(Style::default().bg(bg_select));
    }

    let total = all_lines.len();
    let height = area.height as usize;
    let offset = list_offset.min(total.saturating_sub(1));

    let visible: Vec<Line> = all_lines
        .into_iter()
        .map(|(l, _)| l)
        .skip(offset)
        .take(height)
        .collect();

    let para = Paragraph::new(visible);
    para.render(area, buf);

    // Scrollbar when content overflows.
    if total > height {
        let mut sb_state = ScrollbarState::new(total).position(offset);
        StatefulWidget::render(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            buf,
            &mut sb_state,
        );
    }

    height
}

/// Number of leading lines in the LEVELS tab before the first category entry.
/// Each category occupies exactly 3 lines (header / bar / xp).
/// Used by the nav handler to convert category-index → line-index.
pub const LEVELS_LEAD_LINES: usize = 1;
/// Lines per category entry in the LEVELS tab.
pub const LEVELS_LINES_PER_CAT: usize = 3;

fn render_levels(
    state: &State,
    area: Rect,
    buf: &mut Buffer,
    palette: &Palette,
    truecolor: bool,
    levels_idx: usize,
    levels_offset: usize,
) -> usize {
    use crate::palette::downgrade_color;

    let fg_bright = downgrade_color(palette.fg_bright, truecolor);
    let fg_dim = downgrade_color(palette.fg, truecolor);
    let accent = downgrade_color(palette.accent, truecolor);
    let warm = downgrade_color(palette.warm, truecolor);
    let bg_select = downgrade_color(palette.border_bright, truecolor);
    // Muted skip indicator: fg (palette.fg) — same tone used for dormant/dim text.
    let skip_muted = downgrade_color(palette.fg, truecolor);
    // Warm skip indicator (3+): due color — amber/warm without the panic-red of overdue.
    let skip_warm = downgrade_color(palette.due, truecolor);
    // Integration count: muted gold (warm).
    let integrate_color = warm;
    // Ready-to-integrate affordance: accent.
    let ready_color = accent;

    let now_ms = Utc::now().timestamp_millis();
    let lvl99_xp = xp_for_level(MAX_LEVEL);

    let mut all_lines: Vec<Line> = vec![Line::from(Span::styled(
        "runescape curve · Enter for detail · f = focus",
        Style::default().fg(fg_dim),
    ))];

    for (cat_i, cat) in CATEGORIES.iter().enumerate() {
        let trait_id = seed_core::domain::TraitId(cat.trait_id.to_string());
        let xp = state.traits.get(&trait_id).copied().unwrap_or(0);
        let lvl = level_for_xp(xp);
        let prog = level_progress(xp);
        let to_next = xp_to_next(xp);

        let bar_width = (area.width as usize).saturating_sub(2).max(8);
        let filled = (prog * bar_width as f32).round() as usize;

        // Per-trait color from mandala palette.
        let bar_color = if truecolor {
            trait_color(cat.trait_id, prog)
        } else {
            downgrade_color(trait_color(cat.trait_id, prog), false)
        };

        let bar: String = (0..bar_width)
            .map(|i| if i < filled { '█' } else { '░' })
            .collect();

        // Per-trait skip count for the last 7 days.
        let skip_7d = state
            .traits_skipped
            .get(&trait_id)
            .map(|s| s.count_7d(now_ms))
            .unwrap_or(0);

        // Integration count (B4).
        let integrations = state
            .trait_integrations
            .get(&trait_id)
            .copied()
            .unwrap_or(0);

        // Focus arrows (B3): look up allocation in active focus.
        let focus_arrows: u8 = state
            .active_focus
            .as_ref()
            .and_then(|f| {
                f.allocations
                    .iter()
                    .find(|(t, _)| t.0 == cat.trait_id)
                    .map(|(_, arrows)| *arrows)
            })
            .unwrap_or(0);

        // Ready to integrate (B6).
        let ready_to_integrate = xp >= lvl99_xp;

        let prefix = format!("{} {}", cat.icon, cat.trait_id);
        let suffix = format!("{:>2}/99", lvl);

        // Build right-side annotation string (order: ✦N, ▾N, ▲…, [I]).
        // Each piece is either shown or empty. We compute total width for padding.
        let integrate_str = if integrations > 0 {
            format!(" \u{2726}{integrations}") // ✦N
        } else {
            String::new()
        };
        let skip_str = if skip_7d > 0 {
            format!(" \u{25be}{skip_7d}")
        } else {
            String::new()
        };
        let arrow_str = if focus_arrows > 0 {
            format!(" {}", "\u{25b2}".repeat(focus_arrows as usize)) // ▲ × N
        } else {
            String::new()
        };
        let ready_str = if ready_to_integrate {
            " [I]".to_string()
        } else {
            String::new()
        };

        let annotation_len = integrate_str.chars().count()
            + skip_str.chars().count()
            + arrow_str.chars().count()
            + ready_str.chars().count();
        let pad = (area.width as usize)
            .saturating_sub(prefix.chars().count() + suffix.chars().count() + annotation_len);

        let selected = cat_i == levels_idx;
        let (header_style, row_bg) = if selected {
            (
                Style::default()
                    .fg(fg_bright)
                    .bg(bg_select)
                    .add_modifier(Modifier::BOLD),
                Some(bg_select),
            )
        } else {
            (Style::default().fg(fg_bright), None)
        };

        let skip_color = if skip_7d >= 3 { skip_warm } else { skip_muted };

        let mut header_spans = vec![
            Span::styled(prefix, header_style),
            Span::raw(" ".repeat(pad)),
            Span::styled(suffix, Style::default().fg(accent)),
        ];
        // Append right-side annotations in order: ✦N, ▾N, ▲…, [I]
        if integrations > 0 {
            header_spans.push(Span::styled(
                integrate_str,
                Style::default().fg(integrate_color),
            ));
        }
        if skip_7d > 0 {
            header_spans.push(Span::styled(skip_str, Style::default().fg(skip_color)));
        }
        if focus_arrows > 0 {
            header_spans.push(Span::styled(arrow_str, Style::default().fg(warm)));
        }
        if ready_to_integrate {
            header_spans.push(Span::styled(ready_str, Style::default().fg(ready_color)));
        }
        all_lines.push(Line::from(header_spans));
        // S3: apply bg highlight to bar + xp lines so the whole 3-line group is tinted.
        // `Line::style` patches the background without clobbering per-span foregrounds.
        let bar_line = if let Some(bg) = row_bg {
            Line::from(vec![
                Span::raw("  "),
                Span::styled(bar, Style::default().fg(bar_color)),
            ])
            .style(Style::default().bg(bg))
        } else {
            Line::from(vec![
                Span::raw("  "),
                Span::styled(bar, Style::default().fg(bar_color)),
            ])
        };
        all_lines.push(bar_line);
        let xp_line = if let Some(bg) = row_bg {
            Line::from(Span::styled(
                format!("  {xp} xp  ·  {to_next} to {}", lvl + 1),
                Style::default().fg(fg_dim),
            ))
            .style(Style::default().bg(bg))
        } else {
            Line::from(Span::styled(
                format!("  {xp} xp  ·  {to_next} to {}", lvl + 1),
                Style::default().fg(fg_dim),
            ))
        };
        all_lines.push(xp_line);
    }

    let total = all_lines.len();
    let height = area.height as usize;
    // levels_offset is in line-units (B2/Option A). Clamp to avoid going past content.
    let offset = levels_offset.min(total.saturating_sub(1));

    let visible: Vec<Line> = all_lines.into_iter().skip(offset).take(height).collect();
    let para = Paragraph::new(visible);
    para.render(area, buf);

    if total > height {
        let mut sb_state = ScrollbarState::new(total).position(offset);
        StatefulWidget::render(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            buf,
            &mut sb_state,
        );
    }

    height
}

fn render_log(
    state: &State,
    area: Rect,
    buf: &mut Buffer,
    palette: &Palette,
    truecolor: bool,
    log_offset: usize,
) {
    use crate::palette::downgrade_color;

    let accent = downgrade_color(palette.accent, truecolor);
    let accent2 = downgrade_color(palette.accent2, truecolor);
    let fg_dim = downgrade_color(palette.fg, truecolor);

    let all_log: Vec<_> = state.log.iter().rev().collect();
    let total = all_log.len();
    let height = area.height as usize;
    let offset = log_offset.min(total.saturating_sub(1));

    let lines: Vec<Line> = all_log
        .into_iter()
        .skip(offset)
        .take(height)
        .map(|entry| {
            let color = match entry.tag.as_str() {
                "accent" => accent,
                "accent-2" => accent2,
                _ => fg_dim,
            };
            Line::from(vec![
                Span::styled(format!("{} ", entry.t), Style::default().fg(fg_dim)),
                Span::styled("· ", Style::default().fg(fg_dim)),
                Span::styled(&entry.msg, Style::default().fg(color)),
            ])
        })
        .collect();

    let para = Paragraph::new(lines);
    para.render(area, buf);

    if total > height {
        let mut sb_state = ScrollbarState::new(total).position(offset);
        StatefulWidget::render(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            buf,
            &mut sb_state,
        );
    }
}

#[cfg(test)]
mod tests {
    use seed_core::{
        domain::{CATEGORIES, FocusPattern, FocusPhase, TraitId},
        levels::xp_for_level,
        state::initial_state,
    };

    // -----------------------------------------------------------------------
    // Token-balance chip: logic layer (title bar receives tokens_available)
    // -----------------------------------------------------------------------

    #[test]
    fn tokens_available_hidden_when_zero() {
        let state = initial_state(0);
        let tokens = seed_core::events::tokens_available(&state);
        assert_eq!(tokens, 0, "fresh state has no tokens");
        // TitleBar renders chip only when tokens > 0 — nothing to assert in pure
        // data terms beyond confirming the value is 0.
    }

    #[test]
    fn tokens_available_shown_when_positive() {
        let mut state = initial_state(0);
        state.cumulative_levels_gained = 99;
        let tokens = seed_core::events::tokens_available(&state);
        assert_eq!(tokens, 1, "99 cumulative levels → 1 token");
    }

    // -----------------------------------------------------------------------
    // Active focus arrows: logic layer (B3)
    // -----------------------------------------------------------------------

    #[test]
    fn focus_arrows_zero_when_no_active_focus() {
        let state = initial_state(0);
        let flow = TraitId("flow".into());
        let arrows: u8 = state
            .active_focus
            .as_ref()
            .and_then(|f| {
                f.allocations
                    .iter()
                    .find(|(t, _)| t.0 == "flow")
                    .map(|(_, a)| *a)
            })
            .unwrap_or(0);
        assert_eq!(arrows, 0, "no active focus → 0 arrows on flow");
        let _ = flow; // suppress unused warning
    }

    #[test]
    fn focus_arrows_correct_for_spread3x2() {
        let mut state = initial_state(0);
        // Spread3x2: 3 traits, 1 arrow each.
        let pattern = FocusPattern::Spread3x2;
        let arrows_per = pattern.arrows_per_skill();
        let traits = ["flow", "core", "spine"];
        state.active_focus = Some(FocusPhase {
            pattern,
            allocations: traits
                .iter()
                .map(|&t| (TraitId(t.to_string()), arrows_per))
                .collect(),
        });

        for trait_name in &traits {
            let arrows: u8 = state
                .active_focus
                .as_ref()
                .and_then(|f| {
                    f.allocations
                        .iter()
                        .find(|(t, _)| t.0 == *trait_name)
                        .map(|(_, a)| *a)
                })
                .unwrap_or(0);
            assert_eq!(arrows, 1, "Spread3x2 gives 1 arrow to {trait_name}");
        }

        // Unallocated trait has 0 arrows.
        let arrows_reach: u8 = state
            .active_focus
            .as_ref()
            .and_then(|f| {
                f.allocations
                    .iter()
                    .find(|(t, _)| t.0 == "reach")
                    .map(|(_, a)| *a)
            })
            .unwrap_or(0);
        assert_eq!(arrows_reach, 0, "reach is not in Spread3x2 allocation");
    }

    #[test]
    fn focus_arrows_correct_for_concentrate1x4() {
        let mut state = initial_state(0);
        let pattern = FocusPattern::Concentrate1x4;
        let arrows_per = pattern.arrows_per_skill(); // 3
        state.active_focus = Some(FocusPhase {
            pattern,
            allocations: vec![(TraitId("flow".into()), arrows_per)],
        });
        let arrows: u8 = state
            .active_focus
            .as_ref()
            .and_then(|f| {
                f.allocations
                    .iter()
                    .find(|(t, _)| t.0 == "flow")
                    .map(|(_, a)| *a)
            })
            .unwrap_or(0);
        assert_eq!(arrows, 3, "Concentrate1x4 gives 3 arrows");
    }

    // -----------------------------------------------------------------------
    // Integration count (B4)
    // -----------------------------------------------------------------------

    #[test]
    fn integration_count_zero_by_default() {
        let state = initial_state(0);
        let flow = TraitId("flow".into());
        let count = state.trait_integrations.get(&flow).copied().unwrap_or(0);
        assert_eq!(count, 0);
    }

    #[test]
    fn integration_count_shown_when_positive() {
        let mut state = initial_state(0);
        let flow = TraitId("flow".into());
        *state.trait_integrations.entry(flow.clone()).or_insert(0) = 2;
        let count = state.trait_integrations.get(&flow).copied().unwrap_or(0);
        assert_eq!(count, 2, "integration count should be 2 for flow");
    }

    // -----------------------------------------------------------------------
    // "Ready to integrate" affordance (B6)
    // -----------------------------------------------------------------------

    #[test]
    fn ready_to_integrate_false_when_below_99() {
        let state = initial_state(0);
        let flow = TraitId("flow".into());
        let xp = state.traits.get(&flow).copied().unwrap_or(0);
        let lvl99_xp = xp_for_level(seed_core::levels::MAX_LEVEL);
        assert!(xp < lvl99_xp, "fresh state is below lvl 99");
    }

    #[test]
    fn ready_to_integrate_true_when_at_99() {
        let mut state = initial_state(0);
        let flow = TraitId("flow".into());
        *state.traits.get_mut(&flow).unwrap() = xp_for_level(99);
        let xp = state.traits.get(&flow).copied().unwrap_or(0);
        let lvl99_xp = xp_for_level(seed_core::levels::MAX_LEVEL);
        assert!(xp >= lvl99_xp, "after setting xp=xp_for_level(99) → ready");
    }

    #[test]
    fn ready_to_integrate_false_for_all_other_traits_when_only_flow_at_99() {
        let mut state = initial_state(0);
        *state.traits.get_mut(&TraitId("flow".into())).unwrap() = xp_for_level(99);
        let lvl99_xp = xp_for_level(seed_core::levels::MAX_LEVEL);
        for cat in CATEGORIES {
            if cat.trait_id == "flow" {
                continue;
            }
            let xp = state
                .traits
                .get(&TraitId(cat.trait_id.to_string()))
                .copied()
                .unwrap_or(0);
            assert!(
                xp < lvl99_xp,
                "trait '{}' should not be ready (only flow was set to 99)",
                cat.trait_id
            );
        }
    }
}
