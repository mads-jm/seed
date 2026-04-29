/// Central orbit pane: glyph + 8 orbital reminder cards on an ellipse.
use std::collections::BTreeMap;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Widget},
};
use seed_core::{
    domain::{CATEGORIES, REMINDERS, ReminderState, reminder_status_with_interval, tier_for},
    glyph::render_glyph,
    levels::level_for_xp,
    state::State,
};

use crate::palette::Palette;

pub struct OrbitPane<'a> {
    pub state: &'a State,
    pub tick: u32,
    pub palette: &'a Palette,
    pub truecolor: bool,
    pub braille: bool,
    pub now_ms: i64,
}

/// Data for one orbital reminder card.
struct OrbitCard {
    icon: &'static str,
    name: &'static str,
    word: &'static str,
    pinned: bool,
    state: ReminderState,
    ms_left: i64,
    pct: f32,
    /// Column, row within the buffer (top-left of card).
    col: u16,
    row: u16,
}

impl Widget for OrbitPane<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use crate::palette::downgrade_color;
        use seed_core::glyph::apply_to_buf;

        if area.width < 4 || area.height < 4 {
            return;
        }

        let accent = downgrade_color(self.palette.accent, self.truecolor);
        let fg_dim = downgrade_color(self.palette.fg, self.truecolor);
        let fg_bright = downgrade_color(self.palette.fg_bright, self.truecolor);
        let due_color = downgrade_color(self.palette.due, self.truecolor);
        let overdue_color = downgrade_color(self.palette.overdue, self.truecolor);

        // ── Compute total level / tier ──────────────────────────────────────
        let total_level: u32 = self
            .state
            .traits
            .values()
            .map(|&xp| level_for_xp(xp) as u32)
            .sum();
        let tier = tier_for(total_level);

        // ── Build TraitMap (f32 0..1) ────────────────────────────────────────
        let trait_map: BTreeMap<seed_core::domain::TraitId, f32> = self
            .state
            .traits
            .iter()
            .map(|(k, &xp)| {
                let norm = seed_core::levels::level_norm(xp);
                (k.clone(), norm)
            })
            .collect();

        // ── Card sizing ──────────────────────────────────────────────────────
        // Longest reminder name in the catalog is 13 chars ("MORNING PAGES"); the
        // card needs icon (1) + space (1) + name (≤13) + pin (1) = 16 cells.
        // See docs/specs/glyph-expansion.md (Design C — Orbit card grid layout).
        let card_w: u16 = 16;
        let card_h: u16 = 3;
        let name_chars: usize = (card_w as usize).saturating_sub(3);

        // ── Render glyph (FULL pane) ─────────────────────────────────────────
        // The glyph paints the whole pane area and the cards float on top in
        // their orbit slots — cards are pulled inward (see slot_position) so
        // there's a band of glyph texture rim *outside* the orbit. The
        // ellipse mask + zenith blend in render_glyph handle how aggressively
        // the rim fills with progress: small + central at low progress,
        // pane-filling at zenith.
        let glyph_w = area.width;
        let glyph_h = area.height;

        if glyph_w >= 4 && glyph_h >= 3 {
            let frame = render_glyph(&trait_map, self.state.glyph_seed, (glyph_w, glyph_h));

            let glyph_area = Rect {
                x: area.x,
                y: area.y,
                width: glyph_w,
                height: glyph_h,
            };

            // Post-process cells: downgrade colors and optionally swap braille.
            let processed_frame = if !self.truecolor || !self.braille {
                let mut f = frame.clone();
                for row in &mut f.cells {
                    for cell in row {
                        if !self.truecolor {
                            cell.fg = downgrade_color(cell.fg, false);
                        }
                        if !self.braille {
                            cell.ch = braille_to_block(cell.ch);
                        }
                    }
                }
                f
            } else {
                frame
            };

            // Apply shimmer: on odd ticks swap some high-intensity chars.
            let shimmer_tick = self.tick / 3; // ~6 Hz shimmer at 20 Hz base
            let shimmer_frame = apply_shimmer(processed_frame, shimmer_tick);

            apply_to_buf(&shimmer_frame, buf, glyph_area);
        }

        // ── Pick orbital reminders ────────────────────────────────────────────
        let max_slots = responsive_slot_count(area.width);
        let orbit = pick_orbit_reminders(self.state, self.now_ms, max_slots);

        // ── Card placement (fixed 3×3 grid) ──────────────────────────────────
        // Cards orbit *closer to the core* as progress grows: at total_level=0
        // they sit near the pane edge, at zenith they're pulled well inward.
        // The freed rim fills with mandala texture (driven by progress in the
        // glyph renderer), so growth feels coupled — cards shrink inward while
        // the mandala expands outward. See docs/specs/glyph-expansion.md.
        // 9 traits × max level 99 = 891 total at zenith.
        let orbit_progress = (total_level as f32 / 891.0).clamp(0.0, 1.0);
        let area_fits_cards = area.width >= card_w * 2 && area.height >= card_h * 2;

        let n = orbit.len();
        let cards: Vec<OrbitCard> = if !area_fits_cards {
            Vec::new()
        } else {
            orbit
                .into_iter()
                .enumerate()
                .filter_map(|(i, (reminder, rt, status))| {
                    let (card_col, card_row) =
                        slot_position(i, n, area, card_w, card_h, orbit_progress)?;
                    let cat = CATEGORIES.iter().find(|c| c.id == reminder.cat)?;
                    Some(OrbitCard {
                        icon: cat.icon,
                        name: reminder.name,
                        word: reminder.word,
                        pinned: rt.pinned,
                        state: status.state,
                        ms_left: status.ms_left,
                        pct: status.pct,
                        col: card_col,
                        row: card_row,
                    })
                })
                .collect()
        };

        // ── Draw orbital cards ────────────────────────────────────────────────
        // Each card gets a 1-cell padding ring (cleared empty — gives the card
        // visual breathing room against dense glyph texture) and a solid `bg2`
        // backdrop so the card body never reads through to the mandala behind it.
        let card_bg = downgrade_color(self.palette.bg2, self.truecolor);
        for card in &cards {
            // Padding ring: 1 cell on the horizontal sides only — top/bottom
            // butt directly against surrounding glyph for a tighter silhouette.
            let pad_x = card.col.saturating_sub(1).max(area.x);
            let pad_right = (card.col + card_w + 1).min(area.x + area.width);
            let pad_rect = Rect {
                x: pad_x,
                y: card.row,
                width: pad_right.saturating_sub(pad_x),
                height: card_h,
            };
            Clear.render(pad_rect, buf);

            // Solid card body.
            let card_rect = Rect {
                x: card.col,
                y: card.row,
                width: card_w,
                height: card_h,
            };
            Block::default()
                .style(Style::default().bg(card_bg))
                .render(card_rect, buf);

            let state_color = match card.state {
                ReminderState::Due => due_color,
                ReminderState::Overdue => overdue_color,
                _ => fg_dim,
            };

            let time_str = match card.state {
                ReminderState::Due => "DUE".to_string(),
                ReminderState::Overdue => "OVRD".to_string(),
                ReminderState::Dormant => {
                    let secs = card.ms_left / 1000;
                    let m = secs / 60;
                    let s = secs % 60;
                    format!("{m:02}:{s:02}")
                }
                ReminderState::Off => "OFF".to_string(),
            };

            let pin_mark = if card.pinned { "*" } else { " " };

            // Bar fills the row alongside the time string (4 chars + space = 5).
            let bar_w = (card_w as usize).saturating_sub(6);
            let filled = (card.pct * bar_w as f32).round() as usize;
            let bar: String = (0..bar_w)
                .map(|i| if i < filled { '█' } else { '░' })
                .collect();

            // Row 0: icon + name + pin.
            // Char-based truncation avoids panics on multi-byte UTF-8 names.
            let truncated_name: String = card.name.chars().take(name_chars).collect();
            let row0 = Line::from(vec![Span::styled(
                format!(
                    "{} {:<width$}{}",
                    card.icon,
                    truncated_name,
                    pin_mark,
                    width = name_chars
                ),
                Style::default().fg(fg_bright),
            )]);
            // Row 1: state + bar
            let row1 = Line::from(vec![
                Span::styled(
                    format!("{:>4} ", time_str),
                    Style::default().fg(state_color),
                ),
                Span::styled(bar, Style::default().fg(accent)),
            ]);
            // Row 2: verb hint
            let row2 = Line::from(Span::styled(
                format!(" \"{}\"", card.word),
                Style::default().fg(fg_dim),
            ));

            buf.set_line(card.col, card.row, &row0, card_w);
            buf.set_line(card.col, card.row + 1, &row1, card_w);
            buf.set_line(card.col, card.row + 2, &row2, card_w);
        }

        // ── Tier badge (bottom center) ───────────────────────────────────────
        let badge = format!("{}  {}  LVL {}", tier.name(), tier.adj(), total_level);
        let badge_x = area.x + area.width.saturating_sub(badge.len() as u16) / 2;
        let badge_y = area.y + area.height.saturating_sub(1);
        buf.set_string(badge_x, badge_y, &badge, Style::default().fg(fg_dim));
    }
}

// ---------------------------------------------------------------------------
// Orbit slot picking (ports JSX pickOrbitReminders)
// ---------------------------------------------------------------------------

fn pick_orbit_reminders(
    state: &State,
    now_ms: i64,
    limit: usize,
) -> Vec<(
    &'static seed_core::domain::Reminder,
    &seed_core::state::ReminderRuntime,
    seed_core::domain::ReminderStatus,
)> {
    let enabled: Vec<_> = REMINDERS
        .iter()
        .filter_map(|r| {
            let rt = state.reminders.get(&r.reminder_id())?;
            if !rt.enabled {
                return None;
            }
            let status =
                reminder_status_with_interval(rt.interval_min, rt.last_done_ms, rt.enabled, now_ms);
            Some((r, rt, status))
        })
        .collect();

    let mut pinned: Vec<_> = enabled
        .iter()
        .filter(|(_, rt, _)| rt.pinned)
        .cloned()
        .collect();
    let mut rest: Vec<_> = enabled
        .iter()
        .filter(|(_, rt, _)| !rt.pinned)
        .cloned()
        .collect();

    // Sort rest by urgency: overdue > due > dormant, then by pct desc.
    rest.sort_by(|(_, _, a_st), (_, _, b_st)| {
        let uw = |s: ReminderState| match s {
            ReminderState::Overdue => 3u8,
            ReminderState::Due => 2,
            ReminderState::Dormant => 1,
            ReminderState::Off => 0,
        };
        let dw = uw(b_st.state).cmp(&uw(a_st.state));
        if dw != std::cmp::Ordering::Equal {
            return dw;
        }
        b_st.pct
            .partial_cmp(&a_st.pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Fill pinned first, then urgency-sorted rest, up to limit.
    let mut picked = pinned.drain(..).take(limit).collect::<Vec<_>>();
    for item in rest {
        if picked.len() >= limit {
            break;
        }
        picked.push(item);
    }

    // Stable sort by catalog order.
    picked.sort_by_key(|(r, _, _)| {
        REMINDERS
            .iter()
            .position(|x| x.id == r.id)
            .unwrap_or(usize::MAX)
    });

    picked.truncate(limit);
    picked
}

// ---------------------------------------------------------------------------
// Responsive slot count
// ---------------------------------------------------------------------------

fn responsive_slot_count(pane_width: u16) -> usize {
    if pane_width >= 100 {
        8
    } else if pane_width >= 70 {
        6
    } else {
        4
    }
}

// ---------------------------------------------------------------------------
// Card slot placement (fixed 3×3 grid)
//
// Eight perimeter positions, indexed clockwise from top-centre:
//   0 = TM   1 = TR   2 = MR   3 = BR
//   4 = BM   5 = BL   6 = ML   7 = TL
//
// For slot counts < 8, evenly spaced subsets that preserve top/bottom symmetry:
//   n=4 → [TM, MR, BM, ML]                    (compass)
//   n=6 → [TM, TR, BR, BM, BL, TL]            (4 corners + TM/BM)
//   n=8 → [TM, TR, MR, BR, BM, BL, ML, TL]    (full perimeter)
// ---------------------------------------------------------------------------

/// Top-left coordinate of card slot `idx` of `total`, or `None` if the area
/// can't fit a card of `card_w × card_h`.
///
/// `progress` is `total_level / 891` clamped to 0..=1 — cards orbit closer to
/// the core as progress grows, freeing the rim of the pane for the mandala to
/// paint into. See docs/specs/glyph-expansion.md (Design C).
fn slot_position(
    idx: usize,
    total: usize,
    area: Rect,
    card_w: u16,
    card_h: u16,
    progress: f32,
) -> Option<(u16, u16)> {
    if area.width < card_w || area.height < card_h {
        return None;
    }
    // Inset lerps from a small base (cards near the pane edge at fresh start)
    // to a deep inset (cards huddle closer to the core at zenith). Capped so
    // we never push slots past the centre of the available room.
    let p = progress.clamp(0.0, 1.0);
    let target_inset_h = 4.0 + p * 10.0; // 4..14
    let target_inset_v = 1.0 + p * 2.0; // 1..3
    let inset_h = (target_inset_h.round() as u16).min(area.width.saturating_sub(card_w) / 2);
    let inset_v = (target_inset_v.round() as u16).min(area.height.saturating_sub(card_h) / 2);
    // Map (count, idx) → one of the 8 conceptual positions (0..=7 clockwise from TM).
    const SLOTS_4: [u8; 4] = [0, 2, 4, 6];
    const SLOTS_6: [u8; 6] = [0, 1, 3, 4, 5, 7];
    const SLOTS_8: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
    let pos = match total {
        n if n <= 4 => *SLOTS_4.get(idx)?,
        n if n <= 6 => *SLOTS_6.get(idx)?,
        _ => *SLOTS_8.get(idx)?,
    };
    // (col_kind, row_kind) — 0 = flush start, 1 = centred, 2 = flush end.
    let (col_kind, row_kind) = match pos {
        0 => (1u8, 0u8), // TM
        1 => (2, 0),     // TR
        2 => (2, 1),     // MR
        3 => (2, 2),     // BR
        4 => (1, 2),     // BM
        5 => (0, 2),     // BL
        6 => (0, 1),     // ML
        7 => (0, 0),     // TL
        _ => return None,
    };
    let col = match col_kind {
        0 => area.x + inset_h,
        1 => area.x + area.width.saturating_sub(card_w) / 2,
        2 => area.x + area.width.saturating_sub(card_w + inset_h),
        _ => return None,
    };
    let row = match row_kind {
        0 => area.y + inset_v,
        1 => area.y + area.height.saturating_sub(card_h) / 2,
        2 => area.y + area.height.saturating_sub(card_h + inset_v),
        _ => return None,
    };
    Some((col, row))
}

// ---------------------------------------------------------------------------
// Braille → block density fallback
// ---------------------------------------------------------------------------

/// Replace braille characters with block-density equivalents.
/// Maps the 8-level braille density to block characters of similar visual weight.
pub fn braille_to_block(ch: char) -> char {
    // Popcount-based density: count set bits in the braille bitmask.
    let cp = ch as u32;
    if !(0x2800..=0x28FF).contains(&cp) {
        return ch; // not braille — pass through
    }
    let bits = cp - 0x2800;
    // braille has 8 dot positions; popcount gives density 0..8.
    // Use all 8 distinct output chars to preserve visual fidelity in block fallback.
    let density = bits.count_ones();
    match density {
        0 => ' ',
        1 => '·',
        2 => '░',
        3 => '▀',
        4 => '▒',
        5 => '▄',
        6 => '▓',
        7 => '▊',
        _ => '█',
    }
}

// ---------------------------------------------------------------------------
// Shimmer animation
// ---------------------------------------------------------------------------

/// Apply a tick-driven shimmer: at high intensity cells, occasionally swap
/// to a slightly brighter variant. Purely cosmetic, no flicker risk because
/// it's deterministic from `tick`.
fn apply_shimmer(
    mut frame: seed_core::glyph::GlyphFrame,
    shimmer_tick: u32,
) -> seed_core::glyph::GlyphFrame {
    const SHIMMER_CHARS: &[char] = &['·', '+', '*', '✦', '◇', '◆'];

    for (row_idx, row) in frame.cells.iter_mut().enumerate() {
        for (col_idx, cell) in row.iter_mut().enumerate() {
            if cell.intensity >= 4 {
                // Simple deterministic shimmer: hash position + tick.
                let h = (col_idx as u32)
                    .wrapping_mul(374_761_393)
                    .wrapping_add((row_idx as u32).wrapping_mul(668_265_263))
                    .wrapping_add(shimmer_tick.wrapping_mul(2_654_435_761));
                if h.is_multiple_of(31) {
                    let idx = (h / 31 % SHIMMER_CHARS.len() as u32) as usize;
                    cell.ch = SHIMMER_CHARS[idx];
                }
            }
        }
    }
    frame
}
