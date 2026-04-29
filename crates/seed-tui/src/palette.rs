/// The 5 color palettes as `ratatui::Color::Rgb` tables.
/// Ported from `app.jsx::PALETTES`.
use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub accent: Color,
    pub accent2: Color,
    pub warm: Color,
    pub cool: Color,
    pub bg: Color,
    pub bg2: Color,
    pub fg: Color,
    pub fg_bright: Color,
    pub due: Color,
    pub overdue: Color,
    pub border: Color,
    pub border_bright: Color,
}

impl Palette {
    const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::Rgb(r, g, b)
    }
}

pub const SAGE: Palette = Palette {
    accent: Palette::rgb(0x86, 0xb5, 0xa0),
    accent2: Palette::rgb(0xc9, 0xa0, 0xc4),
    warm: Palette::rgb(0xd4, 0xb6, 0x7f),
    cool: Palette::rgb(0x7f, 0xa8, 0xc4),
    bg: Palette::rgb(0x0e, 0x11, 0x13),
    bg2: Palette::rgb(0x13, 0x17, 0x1a),
    fg: Palette::rgb(0xb8, 0xc2, 0xc8),
    fg_bright: Palette::rgb(0xe8, 0xee, 0xf2),
    due: Palette::rgb(0xd4, 0xb6, 0x7f),
    overdue: Palette::rgb(0xc4, 0x78, 0x78),
    border: Palette::rgb(0x28, 0x34, 0x3a),
    border_bright: Palette::rgb(0x3a, 0x48, 0x52),
};

pub const DUSK: Palette = Palette {
    accent: Palette::rgb(0xb4, 0x8e, 0xc4),
    accent2: Palette::rgb(0xe0, 0xa8, 0x90),
    warm: Palette::rgb(0xd4, 0xa9, 0x6a),
    cool: Palette::rgb(0x8e, 0xa3, 0xc4),
    bg: Palette::rgb(0x10, 0x10, 0x1a),
    bg2: Palette::rgb(0x17, 0x17, 0x23),
    fg: Palette::rgb(0xb8, 0xb4, 0xc8),
    fg_bright: Palette::rgb(0xec, 0xe8, 0xf2),
    due: Palette::rgb(0xd4, 0xa9, 0x6a),
    overdue: Palette::rgb(0xc4, 0x78, 0x78),
    border: Palette::rgb(0x28, 0x28, 0x3a),
    border_bright: Palette::rgb(0x38, 0x38, 0x52),
};

pub const MIST: Palette = Palette {
    accent: Palette::rgb(0xa8, 0xc4, 0xc0),
    accent2: Palette::rgb(0xc4, 0xb8, 0xa8),
    warm: Palette::rgb(0xc8, 0xb8, 0x90),
    cool: Palette::rgb(0x98, 0xb4, 0xc0),
    bg: Palette::rgb(0x12, 0x16, 0x1a),
    bg2: Palette::rgb(0x18, 0x1e, 0x23),
    fg: Palette::rgb(0xb4, 0xc0, 0xc4),
    fg_bright: Palette::rgb(0xe4, 0xee, 0xf2),
    due: Palette::rgb(0xc8, 0xb8, 0x90),
    overdue: Palette::rgb(0xc4, 0x78, 0x78),
    border: Palette::rgb(0x28, 0x30, 0x36),
    border_bright: Palette::rgb(0x38, 0x44, 0x4c),
};

pub const EMBER: Palette = Palette {
    accent: Palette::rgb(0xd0, 0x98, 0x78),
    accent2: Palette::rgb(0xa8, 0x90, 0x90),
    warm: Palette::rgb(0xd4, 0xa8, 0x68),
    cool: Palette::rgb(0x9c, 0x88, 0x78),
    bg: Palette::rgb(0x14, 0x0f, 0x0c),
    bg2: Palette::rgb(0x1a, 0x16, 0x12),
    fg: Palette::rgb(0xc0, 0xb0, 0xa0),
    fg_bright: Palette::rgb(0xf0, 0xe4, 0xd8),
    due: Palette::rgb(0xd4, 0xa8, 0x68),
    overdue: Palette::rgb(0xc4, 0x78, 0x78),
    border: Palette::rgb(0x30, 0x28, 0x22),
    border_bright: Palette::rgb(0x44, 0x38, 0x30),
};

pub const MOSS: Palette = Palette {
    accent: Palette::rgb(0x92, 0xa8, 0x7c),
    accent2: Palette::rgb(0xb8, 0xa0, 0x70),
    warm: Palette::rgb(0xc4, 0xac, 0x70),
    cool: Palette::rgb(0x8c, 0xa8, 0x94),
    bg: Palette::rgb(0x0f, 0x12, 0x0e),
    bg2: Palette::rgb(0x16, 0x1a, 0x13),
    fg: Palette::rgb(0xb8, 0xc0, 0xa8),
    fg_bright: Palette::rgb(0xe8, 0xee, 0xd8),
    due: Palette::rgb(0xc4, 0xac, 0x70),
    overdue: Palette::rgb(0xc4, 0x78, 0x78),
    border: Palette::rgb(0x28, 0x30, 0x22),
    border_bright: Palette::rgb(0x38, 0x44, 0x30),
};

/// Resolve palette by name. Falls back to sage for unknown names.
pub fn palette_for(name: &str) -> &'static Palette {
    match name {
        "dusk" => &DUSK,
        "mist" => &MIST,
        "ember" => &EMBER,
        "moss" => &MOSS,
        _ => &SAGE,
    }
}

/// Downgrade a `Color::Rgb` to the nearest xterm-256 color when truecolor
/// is not supported. Non-RGB colors pass through unchanged.
pub fn downgrade_color(c: Color, truecolor: bool) -> Color {
    if truecolor {
        return c;
    }
    match c {
        Color::Rgb(r, g, b) => rgb_to_256(r, g, b),
        other => other,
    }
}

/// Map (r, g, b) to the nearest xterm-256 color index.
/// Uses the 6×6×6 color cube (indices 16–231) for chromatic colors and
/// the 24-step grayscale ramp (indices 232–255) for near-neutral values.
fn rgb_to_256(r: u8, g: u8, b: u8) -> Color {
    // Check grayscale ramp first (within 10 units of neutral).
    let avg = (r as u16 + g as u16 + b as u16) / 3;
    let max_diff = [
        r as i16 - avg as i16,
        g as i16 - avg as i16,
        b as i16 - avg as i16,
    ]
    .iter()
    .map(|d| d.unsigned_abs())
    .max()
    .unwrap_or(0);

    if max_diff < 12 {
        // Map to grayscale ramp 232-255 (24 steps: 8,18,28..238).
        let idx = ((avg.saturating_sub(8)) / 10).min(23) as u8;
        return Color::Indexed(232 + idx);
    }

    // Map to 6×6×6 color cube (indices 16-231).
    let ri = ((r as u16 * 5 + 127) / 255) as u8;
    let gi = ((g as u16 * 5 + 127) / 255) as u8;
    let bi = ((b as u16 * 5 + 127) / 255) as u8;
    Color::Indexed(16 + ri * 36 + gi * 6 + bi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downgrade_passthrough_when_truecolor() {
        let c = Color::Rgb(0x86, 0xb5, 0xa0);
        assert_eq!(downgrade_color(c, true), c);
    }

    #[test]
    fn downgrade_rgb_to_indexed_when_no_truecolor() {
        let c = Color::Rgb(0x86, 0xb5, 0xa0);
        let d = downgrade_color(c, false);
        assert!(
            matches!(d, Color::Indexed(_)),
            "expected Indexed, got {d:?}"
        );
    }

    #[test]
    fn downgrade_non_rgb_unchanged() {
        let c = Color::White;
        assert_eq!(downgrade_color(c, false), Color::White);
    }

    #[test]
    fn downgrade_black_uses_grayscale_ramp() {
        // (0,0,0) → grayscale ramp index 232+0 = 232
        let d = downgrade_color(Color::Rgb(0, 0, 0), false);
        assert_eq!(d, Color::Indexed(232));
    }

    #[test]
    fn palette_for_unknown_returns_sage() {
        let p = palette_for("unknown");
        // Compare accent color, not pointer (statics may not have same addr in tests).
        assert_eq!(p.accent, SAGE.accent);
        assert_eq!(p.bg, SAGE.bg);
    }

    #[test]
    fn all_named_palettes_resolve() {
        for name in &["sage", "dusk", "mist", "ember", "moss"] {
            let _ = palette_for(name);
        }
    }
}
