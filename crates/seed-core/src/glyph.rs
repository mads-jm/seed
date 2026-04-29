//! Glyph renderer — generative ASCII mandala that grows from a sparse seed
//! (low total level) to a pane-filling, multi-hued mandala at zenith.
//!
//! Ports `wellness/glyph.jsx` faithfully:
//! - same layer composition order
//! - same hash function (adapted to full u64 arithmetic via splitmix)
//! - same weighted-char distribution with identical bias rules
//! - same symmetry mirroring (H then V) — unconditional: dominant side always wins
//! - same aura/halo unlock thresholds (0.4 / 0.7 progress)
//!
//! Pure — no I/O, no randomness. Same `(traits, seed, target)` → byte-equal `GlyphFrame`.
//!
//! # Color blending
//! Per-layer base hue is stored as HSL and converted to RGB via a small inline
//! implementation (no external dep). Every layer writes to its cells unconditionally;
//! when layers overlap the colors are blended: new layer gets 0.40 weight against the
//! existing accumulated color. The `layer` field records whichever layer has contributed
//! the most weight (dominant contributor). This ensures outer layers (Warmth, Depth, Halo)
//! always produce at least one visible cell at zenith.

use std::collections::BTreeMap;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier},
};

use crate::domain::TraitId;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A rendered cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphCell {
    pub ch: char,
    pub layer: Option<Layer>,
    /// Per-cell 24-bit colour.
    pub fg: Color,
    /// 0..=5, mirrors `intensityClass` from JSX.
    pub intensity: u8,
}

/// Structural layer a cell belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Core,
    Clarity,
    Reach,
    Spine,
    Flow,
    Space,
    Depth,
    Resonance,
    Warmth,
    Aura,
    Halo,
}

/// A fully-rendered glyph frame.
#[derive(Debug, Clone)]
pub struct GlyphFrame {
    /// `cells[row][col]`
    pub cells: Vec<Vec<GlyphCell>>,
    pub cols: u16,
    pub rows: u16,
    /// 0..=5, derived from total progress.
    pub intensity_idx: u8,
    /// `true` iff all 9 traits ≥ 0.97.
    pub zenith: bool,
    /// 0.0..=1.0 average of all trait values.
    pub progress: f32,
}

/// Normalised trait map: BTreeMap<TraitId, f32> where each value is 0.0..=1.0.
pub type TraitMap = BTreeMap<TraitId, f32>;

// ---------------------------------------------------------------------------
// Character palettes
// ---------------------------------------------------------------------------

/// Inner core: braille block density spectrum (low → high density).
/// Selected to span the full braille density range.
const CORE_CHARS: &[char] = &[
    '\u{2800}', // ⠀ blank braille
    '\u{2801}', // ⠁
    '\u{2809}', // ⠉
    '\u{2819}', // ⠙
    '\u{283B}', // ⠻
    '\u{28B6}', // ⢶
    '\u{28FF}', // ⣿ full braille
    '\u{28FF}', // ⣿ doubled to weight top end
];

/// Clarity (concentric rings) — braille for tight structural detail.
const RING_CHARS: &[char] = &[
    '\u{2800}', // ⠀
    '\u{2802}', // ⠂
    '\u{2812}', // ⠒
    '\u{283A}', // ⠺
    '\u{28BE}', // ⢾
    '\u{28F7}', // ⣷
    '\u{28FF}', // ⣿
    '\u{28FF}', // ⣿
];

/// Flow — cool wave characters.
const FLOW_CHARS: &[char] = &[' ', '.', '~', '-', '/', '\\', '=', '*'];

/// Spine — box-drawing vertical axis.
const SPINE_CHARS: &[char] = &['│', '┃', '╿', '╽', '╳', '┼', '┼', '┼'];

/// Reach arms + branches — diagonal + curve characters.
const REACH_CHARS: &[char] = &[' ', '.', '\'', '╱', '╲', '╳', '╪', '╫', '╭'];

/// Space — mostly blank with occasional dots.
const SPACE_CHARS: &[char] = &[' ', ' ', '.', ' ', ':', ' ', '.', ' '];

/// Depth — violet outer journal bands.
const DEPTH_CHARS: &[char] = &[' ', '.', ',', ';', ':', '=', '#', '&'];

/// Resonance — scattered sparks.
const RES_CHARS: &[char] = &[' ', '.', '*', '+', 'x', 'X', '#', '@'];

/// Warmth — petal punctuation, full half-block set per backlog AC.
const WARM_CHARS: &[char] = &[
    ' ', '.', '▖', '▗', '▘', '▝', '▄', '▌', '▐', '▙', '▚', '▛', '▜', '▞', '▟', '▒', '▓', '█',
];

/// Aura — far-field wash, block half-tones (extended per backlog AC).
const AURA_CHARS: &[char] = &[' ', '.', '░', '▒', '▄', '▌', '▐', '`', '.', '\''];

/// Halo — outer warm punctuation, mix of block + box (extended per backlog AC).
const HALO_CHARS: &[char] = &[' ', '.', ':', 'o', '▖', '▗', '▘', '▝', 'O', '*', '+', '#'];

// ---------------------------------------------------------------------------
// SIZE_STEPS: [w, h, intensity_idx]
// At progress=0 → step 0 (25×13); at progress=1 → step 5 (91×45, or target).
// ---------------------------------------------------------------------------

const SIZE_STEPS: &[(u16, u16, u8)] = &[
    (25, 13, 0),
    (35, 19, 1),
    (49, 25, 2),
    (63, 31, 3),
    (79, 39, 4),
    (91, 45, 5),
];

// ---------------------------------------------------------------------------
// Trait key constants — must match domain.rs catalog ids.
// ---------------------------------------------------------------------------
const TRAIT_FLOW: &str = "flow";
const TRAIT_CORE: &str = "core";
const TRAIT_SPINE: &str = "spine";
const TRAIT_REACH: &str = "reach";
const TRAIT_CLARITY: &str = "clarity";
const TRAIT_SPACE: &str = "space";
const TRAIT_DEPTH: &str = "depth";
const TRAIT_RESONANCE: &str = "resonance";
const TRAIT_WARMTH: &str = "warmth";

// ---------------------------------------------------------------------------
// Deterministic hash — ports JSX `hash(x, y, s)` exactly.
// Returns value in [0.0, 1.0).
// ---------------------------------------------------------------------------

/// Ports `hash(x, y, s)` from glyph.jsx, extended to use the full u64 seed.
/// The lower 32 bits match the JSX exactly; the upper 32 bits are folded in via
/// splitmix so that seeds differing only in the high word produce distinct output.
/// Input coordinates are i32 to handle negative values during rendering.
#[inline]
fn cell_hash(x: i32, y: i32, s: u64) -> f32 {
    // Fold full u64 seed: mix high/low 32-bit halves via splitmix step.
    let s_lo = s as u32;
    let s_hi = (s >> 32) as u32;
    let s32 = s_lo
        .wrapping_add(s_hi.wrapping_mul(2_654_435_761))
        .wrapping_mul(2_147_483_647);

    // Cast coordinates to u32 with wrapping to match JS `>>> 0` (unsigned 32-bit wrap).
    let xi = x as u32;
    let yi = y as u32;
    // JS: (x * 374761393 + y * 668265263 + s * 2147483647) >>> 0
    let h = xi
        .wrapping_mul(374_761_393)
        .wrapping_add(yi.wrapping_mul(668_265_263))
        .wrapping_add(s32);
    // JS: (h ^ (h >>> 13)) * 1274126177 (as u32)
    let h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    // JS: (h ^ (h >>> 16)) >>> 0
    let h = h ^ (h >> 16);
    (h % 10_000) as f32 / 10_000.0
}

// ---------------------------------------------------------------------------
// Weighted char picker — ports JSX `weightedChar(v, arr, h)` exactly.
// ---------------------------------------------------------------------------

/// Ports `weightedChar(v, arr, h)` from glyph.jsx.
/// `h` is a deterministic hash value in [0.0, 1.0).
fn weighted_char(v: f32, palette: &[char], h: f32) -> char {
    let n = palette.len() as f32;
    let center = v * (n - 1.0);
    // Asymmetric spread: wider at low v, tighter at high v but never zero.
    let spread = 1.2 + (1.0 - v) * 2.5;
    // Triangle-like pick: map h in 0..1 through biased curve.
    let u = h * 2.0 - 1.0; // -1..1
    // Bias toward center (u^1.9 keeps mass near 0)
    let sign = if u >= 0.0 { 1.0_f32 } else { -1.0_f32 };
    let biased = sign * u.abs().powf(1.9);
    let mut idx = (center + biased * spread).round() as i32;
    // At high v, occasionally dip LOW for texture (5% chance)
    if v > 0.7 && h < 0.05 {
        idx = (n * 0.35 + h * n * 0.3) as i32;
    }
    // At low v, occasionally poke HIGH (1-2% chance) for sparse hints
    if v < 0.3 && h > 0.985 {
        idx = (n * 0.7) as i32;
    }
    idx = idx.max(0).min(palette.len() as i32 - 1);
    palette[idx as usize]
}

// ---------------------------------------------------------------------------
// Color system — HSL → RGB conversion inline, no external dep.
// ---------------------------------------------------------------------------

/// Convert HSL (h: 0..360, s: 0..1, l: 0..1) → (r, g, b) in 0..=255.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    if s == 0.0 {
        let v = (l * 255.0) as u8;
        return (v, v, v);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let h = h / 360.0;
    let r = hue_to_rgb(p, q, h + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 1.0 / 3.0);
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

#[inline]
fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 1.0 / 2.0 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

/// Blend two RGB colours with weight `t` (0.0 = full a, 1.0 = full b).
#[inline]
fn blend_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    (
        (a.0 as f32 * (1.0 - t) + b.0 as f32 * t) as u8,
        (a.1 as f32 * (1.0 - t) + b.1 as f32 * t) as u8,
        (a.2 as f32 * (1.0 - t) + b.2 as f32 * t) as u8,
    )
}

/// Per-layer color. Returns Color::Rgb blended from base hue toward accent.
/// `trait_v` is the trait value for this layer (0..1), `intensity` is 0..5.
fn layer_color(layer: Layer, trait_v: f32, intensity: u8) -> Color {
    // Base and accent hues for each layer (H in 0..360, S, L).
    // Sage palette: warm amber core, cool blue/cyan flow, violet depth.
    let (base_h, base_s, base_l, acc_h, acc_s, acc_l) = match layer {
        Layer::Core => (38.0, 0.85, 0.55, 24.0, 0.9, 0.65), // amber → orange
        Layer::Clarity => (195.0, 0.7, 0.60, 210.0, 0.8, 0.70), // cyan accent
        Layer::Flow => (205.0, 0.75, 0.55, 185.0, 0.8, 0.65), // cool blue → cyan
        Layer::Spine => (65.0, 0.7, 0.72, 80.0, 0.8, 0.82), // bright lime
        Layer::Reach => (155.0, 0.6, 0.60, 170.0, 0.7, 0.70), // sage green → teal
        Layer::Space => (0.0, 0.0, 0.35, 0.0, 0.0, 0.45),   // near-black
        Layer::Depth => (270.0, 0.55, 0.45, 285.0, 0.65, 0.55), // violet
        Layer::Resonance => (45.0, 0.90, 0.65, 55.0, 0.95, 0.75), // warm sparks
        Layer::Warmth => (25.0, 0.80, 0.60, 15.0, 0.85, 0.70), // warm peach/rose
        Layer::Aura => (185.0, 0.60, 0.65, 195.0, 0.70, 0.75), // far cyan
        Layer::Halo => (40.0, 0.75, 0.70, 50.0, 0.85, 0.80), // warm gold
    };

    // Blend intensity: trait value pushes toward accent; intensity_idx brightens.
    let brightness_boost = (intensity as f32 / 5.0) * 0.08;
    let blend_t = (trait_v * 0.65 + intensity as f32 / 5.0 * 0.35).clamp(0.0, 1.0);

    let base_rgb = hsl_to_rgb(base_h, base_s, (base_l + brightness_boost).min(1.0));
    let acc_rgb = hsl_to_rgb(acc_h, acc_s, (acc_l + brightness_boost).min(1.0));
    let (r, g, b) = blend_rgb(base_rgb, acc_rgb, blend_t);
    Color::Rgb(r, g, b)
}

// ---------------------------------------------------------------------------
// Internal grid cell during rendering
// ---------------------------------------------------------------------------

/// Per-cell state during rendering.  Color is accumulated as a weighted RGB
/// blend so that every layer contributes visibly even when it overlaps inner layers.
#[derive(Clone)]
struct GridCell {
    ch: char,
    layer: Option<Layer>,
    /// Accumulated blended colour (linear RGB, 0.0..=255.0 range).
    blend_r: f32,
    blend_g: f32,
    blend_b: f32,
    /// Total weight accumulated so far (starts at 0.0 for an empty cell).
    blend_w: f32,
}

impl Default for GridCell {
    fn default() -> Self {
        GridCell {
            ch: ' ',
            layer: None,
            blend_r: 0.0,
            blend_g: 0.0,
            blend_b: 0.0,
            blend_w: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Trait value extraction helpers
// ---------------------------------------------------------------------------

fn tv(traits: &TraitMap, key: &str) -> f32 {
    let v = traits.get(&TraitId(key.to_owned())).copied().unwrap_or(0.0);
    // f32::NAN.clamp(0.0, 1.0) returns NaN in Rust — guard explicitly.
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

// ---------------------------------------------------------------------------
// pick_size: picks [w, h, intensity_idx] based on progress and target.
// Ports JSX `pickSize` + target-aware growth logic.
// ---------------------------------------------------------------------------

fn pick_size(progress: f32, target: (u16, u16)) -> (u16, u16, u8) {
    // Exponential curve — same as JSX: curved = progress^0.6
    let curved = progress.powf(0.6);
    let raw_idx = (curved * SIZE_STEPS.len() as f32) as usize;
    let idx = raw_idx.min(SIZE_STEPS.len() - 1);
    let (base_w, base_h, intensity_idx) = SIZE_STEPS[idx];

    let (tw, th) = target;
    if tw == 0 || th == 0 {
        return (base_w, base_h, intensity_idx);
    }

    // Target-aware: grow grid to fill target, capped at 160×80.
    let max_w: u16 = tw.min(160);
    let max_h: u16 = th.min(80);

    let grown_w = ((max_w as f32 * progress.powf(0.5)) as u16)
        .max(base_w)
        .min(max_w);
    let grown_h = ((max_h as f32 * progress.powf(0.5)) as u16)
        .max(base_h)
        .min(max_h);

    // Always odd so center is exact.
    let grid_w = if grown_w % 2 == 0 {
        grown_w.saturating_sub(1)
    } else {
        grown_w
    };
    let grid_h = if grown_h % 2 == 0 {
        grown_h.saturating_sub(1)
    } else {
        grown_h
    };

    (grid_w.max(1), grid_h.max(1), intensity_idx)
}

// ---------------------------------------------------------------------------
// Rendering grid helpers — both blend new color into the existing cell.
//
// Blending strategy: new layer contributes at NEW_LAYER_WEIGHT (0.40) against
// the accumulated color. The `layer` field records the dominant contributor
// (the layer whose weights sum highest, approximated by last-writer-of-the-char).
// `put_blend` always writes the character; `fill_blend` only writes the char
// when the cell is currently empty (space) — but BOTH always blend color.
//
// This ensures outer layers (Warmth/Depth/Halo) register color even when inner
// layers have already filled every cell with non-space chars.
// ---------------------------------------------------------------------------

const NEW_LAYER_WEIGHT: f32 = 0.40;

#[inline]
fn blend_into(
    cell: &mut GridCell,
    ch: char,
    layer: Option<Layer>,
    color: (u8, u8, u8),
    write_ch: bool,
) {
    if write_ch {
        cell.ch = ch;
        cell.layer = layer;
    }
    // Always blend color regardless of whether we wrote the char.
    let (nr, ng, nb) = color;
    let new_w = NEW_LAYER_WEIGHT;
    let old_w = 1.0 - new_w;
    if cell.blend_w == 0.0 {
        // First writer — set color and layer directly.
        cell.blend_r = nr as f32;
        cell.blend_g = ng as f32;
        cell.blend_b = nb as f32;
        cell.blend_w = 1.0;
        if !write_ch && cell.layer.is_none() {
            // Blend-only path: record this layer as dominant since it's the first.
            cell.layer = layer;
        }
    } else {
        cell.blend_r = cell.blend_r * old_w + nr as f32 * new_w;
        cell.blend_g = cell.blend_g * old_w + ng as f32 * new_w;
        cell.blend_b = cell.blend_b * old_w + nb as f32 * new_w;
        // Update dominant layer: if this new layer contributes a significant
        // fraction of total accumulated weight, it becomes the dominant layer.
        // Threshold: new contribution is at least 35% of accumulated weight.
        let total_w = cell.blend_w + new_w;
        if new_w / total_w >= 0.35 && !write_ch {
            cell.layer = layer;
        }
        cell.blend_w = total_w;
    }
}

/// Always writes char + blends color.
/// A space char is ignored — it means "no glyph element here".
/// Use `GridCell::default()` directly for space-punch (erasing cells).
#[inline]
fn put_blend(
    grid: &mut [Vec<GridCell>],
    x: i32,
    y: i32,
    ch: char,
    layer: Option<Layer>,
    color: (u8, u8, u8),
) {
    if ch == ' ' {
        return; // Space from weighted_char means "nothing to draw".
    }
    if x >= 0 && y >= 0 {
        let (x, y) = (x as usize, y as usize);
        if y < grid.len() && x < grid[y].len() {
            blend_into(&mut grid[y][x], ch, layer, color, true);
        }
    }
}

/// Writes char only if cell is truly empty (ch==' ' AND layer=None).
/// Space-punched cells (ch==' ', layer=Some(Space)) are treated as occupied
/// so `fill_blend` does not overwrite them.
/// A space char is ignored — it means "no glyph element here".
#[inline]
fn fill_blend(
    grid: &mut [Vec<GridCell>],
    x: i32,
    y: i32,
    ch: char,
    layer: Option<Layer>,
    color: (u8, u8, u8),
) {
    if ch == ' ' {
        return; // Space from weighted_char means "nothing to draw".
    }
    if x >= 0 && y >= 0 {
        let (x, y) = (x as usize, y as usize);
        if y < grid.len() && x < grid[y].len() {
            // Only write char when cell is truly empty (not space-punched).
            let write_ch = grid[y][x].ch == ' ' && grid[y][x].layer.is_none();
            blend_into(&mut grid[y][x], ch, layer, color, write_ch);
        }
    }
}

// ---------------------------------------------------------------------------
// Main renderer
// ---------------------------------------------------------------------------

/// Render the glyph for the given trait map, seed, and canvas size.
///
/// # Panics
/// Never panics. Returns an empty frame when `target == (0, 0)`.
pub fn render_glyph(traits: &TraitMap, seed: u64, target: (u16, u16)) -> GlyphFrame {
    // --- Early-out for zero target ---
    if target.0 == 0 || target.1 == 0 {
        return GlyphFrame {
            cells: vec![],
            cols: 0,
            rows: 0,
            intensity_idx: 0,
            zenith: false,
            progress: 0.0,
        };
    }

    // --- Trait extraction ---
    let flow = tv(traits, TRAIT_FLOW);
    let core = tv(traits, TRAIT_CORE);
    let spine = tv(traits, TRAIT_SPINE);
    let reach = tv(traits, TRAIT_REACH);
    let clarity = tv(traits, TRAIT_CLARITY);
    let space = tv(traits, TRAIT_SPACE);
    let depth = tv(traits, TRAIT_DEPTH);
    let resonance = tv(traits, TRAIT_RESONANCE);
    let warmth = tv(traits, TRAIT_WARMTH);

    let progress = ((flow + core + spine + reach + clarity + space + depth + resonance + warmth)
        / 9.0)
        .clamp(0.0, 1.0);

    // Zenith: all 9 traits >= 0.97
    let zenith = [
        flow, core, spine, reach, clarity, space, depth, resonance, warmth,
    ]
    .iter()
    .all(|&v| v >= 0.97);

    let (grid_w, grid_h, intensity_idx) = pick_size(progress, target);

    let gw = grid_w as usize;
    let gh = grid_h as usize;

    // Allocate grid.
    let mut grid: Vec<Vec<GridCell>> = vec![vec![GridCell::default(); gw]; gh];

    // Geometric constants — all f32 for renderer arithmetic.
    let cx = (gw as f32 - 1.0) / 2.0;
    let cy = (gh as f32 - 1.0) / 2.0;
    let max_r = (cx * cx + cy * cy).sqrt();
    // Scale relative to the reference 27-wide mandala (matches JSX `scale = GRID_W / 27`).
    let scale = gw as f32 / 27.0;

    // Elliptical normalised radius and falloff mask: r_ellipse = 0 at center,
    // 1 at ellipse boundary, > 1 beyond (the rectangular corners). Outer scatter
    // passes use r_ellipse for outer-band gating AND multiply density by a
    // (1 - r²) falloff so cells fade smoothly to zero at the boundary and
    // never paint the corners. See docs/specs/glyph-expansion.md.
    // dx_max / dy_max use the same 0.58 aspect correction the renderer applies elsewhere.
    let ellipse_dx_max = (cx * 0.58).max(0.001);
    let ellipse_dy_max = cy.max(0.001);
    let r_ellipse = |dx: f32, dy: f32| -> f32 {
        let nx = dx / ellipse_dx_max;
        let ny = dy / ellipse_dy_max;
        (nx * nx + ny * ny).sqrt()
    };
    // Zenith blend: 0 below progress=0.85, ramps via smoothstep up to a
    // CAPPED maximum at progress=1.0. The cap (`ZENITH_CAP`) intentionally
    // leaves headroom for prestige / integration visuals — vanilla level 99
    // across all 9 traits should feel like a complete-but-not-overwhelming
    // mandala, not a corner-filled blowout. Integration unlocks (planned)
    // will lift the cap and add solid block-form layers on top.
    const ZENITH_CAP: f32 = 0.5;
    let zenith_blend = {
        let t = ((progress - 0.85) / 0.15).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t) * ZENITH_CAP
    };
    // ellipse_mask: 1 at centre, falls to 0 at the elliptical boundary and beyond.
    // At zenith_blend=1 the mask is identically 1 everywhere — corners paint at
    // full density.
    let ellipse_mask = |dx: f32, dy: f32| -> f32 {
        let nx = dx / ellipse_dx_max;
        let ny = dy / ellipse_dy_max;
        let r2 = nx * nx + ny * ny;
        let base = (1.0 - r2).max(0.0);
        (base + zenith_blend * (1.0 - base)).min(1.0)
    };
    // Outer cap on re used by Aura: 1.0 normally, expanding to ~1.5 at zenith
    // so the aura band can reach the rectangular corners. (sqrt(2) ≈ 1.414 is
    // the corner radius for a square ellipse.)
    let aura_re_max = 1.0 + zenith_blend * 0.55;

    // Pre-compute per-layer colors once (trait values are constant per frame).
    let c_core = layer_color(Layer::Core, core, intensity_idx);
    let c_clarity = layer_color(Layer::Clarity, clarity, intensity_idx);
    let c_reach = layer_color(Layer::Reach, reach, intensity_idx);
    let c_spine = layer_color(Layer::Spine, spine, intensity_idx);
    let c_flow = layer_color(Layer::Flow, flow, intensity_idx);
    let c_space = layer_color(Layer::Space, space, intensity_idx);
    let c_depth = layer_color(Layer::Depth, depth, intensity_idx);
    let c_resonance = layer_color(Layer::Resonance, resonance, intensity_idx);
    let c_warmth = layer_color(Layer::Warmth, warmth, intensity_idx);
    let c_aura = layer_color(Layer::Aura, progress, intensity_idx);
    let c_halo = layer_color(Layer::Halo, progress, intensity_idx);

    /// Extract (r,g,b) tuple from a ratatui Color::Rgb.  Other variants return (0,0,0).
    #[inline]
    fn rgb(c: Color) -> (u8, u8, u8) {
        if let Color::Rgb(r, g, b) = c {
            (r, g, b)
        } else {
            (0, 0, 0)
        }
    }

    // ---- FRACTAL MICRO CORE: sub-pixel detail at very center ----
    for y in 0..gh {
        for x in 0..gw {
            let dx = (x as f32 - cx) * 0.58;
            let dy = y as f32 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            if d < 1.2 {
                let h = cell_hash(x as i32, y as i32, seed.wrapping_add(211));
                if h < 0.55 {
                    let ch = weighted_char(core, CORE_CHARS, h);
                    put_blend(
                        &mut grid,
                        x as i32,
                        y as i32,
                        ch,
                        Some(Layer::Core),
                        rgb(c_core),
                    );
                }
            }
        }
    }

    // ---- CORE ----
    let core_radius = (1.0 + core * 3.0) * scale;
    for y in 0..gh {
        for x in 0..gw {
            let dx = (x as f32 - cx) * 0.58;
            let dy = y as f32 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            if d < core_radius {
                let h = cell_hash(x as i32, y as i32, seed.wrapping_add(1));
                if d < 0.6 {
                    let ch = weighted_char(core, CORE_CHARS, h);
                    put_blend(
                        &mut grid,
                        x as i32,
                        y as i32,
                        ch,
                        Some(Layer::Core),
                        rgb(c_core),
                    );
                } else if d < core_radius - 0.3 && h < 0.72 {
                    let v = core * (1.0 - d / core_radius * 0.35);
                    let ch = weighted_char(v, CORE_CHARS, h);
                    put_blend(
                        &mut grid,
                        x as i32,
                        y as i32,
                        ch,
                        Some(Layer::Core),
                        rgb(c_core),
                    );
                }
            }
        }
    }

    // ---- CLARITY rings: many concentric rings at higher level ----
    let ring_count = (1.0 + (clarity * 4.0).round()) as usize;
    for r in 0..ring_count {
        let ring_r1 = (3.2 + clarity * 1.4 + r as f32 * 2.2) * scale;
        let ring_r2 = ring_r1 + 1.1 * scale;
        let ring_density = (0.35 + clarity * 0.55 - r as f32 * 0.04).max(0.0);
        for y in 0..gh {
            for x in 0..gw {
                let dx = (x as f32 - cx) * 0.58;
                let dy = y as f32 - cy;
                let d = (dx * dx + dy * dy).sqrt();
                if d > ring_r1 && d < ring_r2 {
                    let h = cell_hash(
                        x as i32,
                        y as i32,
                        seed.wrapping_add(7).wrapping_add(r as u64 * 13),
                    );
                    if h < ring_density {
                        let v = clarity * (1.0 - r as f32 * 0.08);
                        let ch = weighted_char(v, RING_CHARS, h);
                        put_blend(
                            &mut grid,
                            x as i32,
                            y as i32,
                            ch,
                            Some(Layer::Clarity),
                            rgb(c_clarity),
                        );
                    }
                }
            }
        }
    }

    // ---- REACH rays + FRACTAL MACRO TENDRILS ----
    let arm_count = (reach * 16.0).round().max(0.0) as usize;
    let arm_len = (4.0 + reach * 14.0) * scale;
    for i in 0..arm_count {
        let a = (i as f32 / arm_count as f32) * std::f32::consts::TAU + (seed % 7) as f32 * 0.1;
        let mut step_idx: usize = 0;
        let mut t = 1.2_f32;
        while t < arm_len {
            step_idx += 1;
            let x = (cx + a.cos() * t / 0.58).round() as i32;
            let y = (cy + a.sin() * t * 0.9).round() as i32;
            let h = cell_hash(x, y, seed.wrapping_add(17));
            let v = reach * (1.0 - t / arm_len * 0.7).max(0.35);
            let ch = weighted_char(v, REACH_CHARS, h);
            fill_blend(&mut grid, x, y, ch, Some(Layer::Reach), rgb(c_reach));

            // Branches every 4 steps (perpendicular) when reach is high enough.
            if reach > 0.5 && step_idx.is_multiple_of(4) && t > 3.0 {
                let perp = a + std::f32::consts::FRAC_PI_2;
                // Cap branch length to prevent excessive iteration at high reach.
                let branch_len = (t * 0.35).min(arm_len * 0.2).min(12.0);
                let mut bt = 0.6_f32;
                while bt < branch_len {
                    for sign in [-1.0_f32, 1.0] {
                        let bx = (x as f32 + perp.cos() * bt * sign / 0.58).round() as i32;
                        let by = (y as f32 + perp.sin() * bt * sign * 0.9).round() as i32;
                        let bh = cell_hash(bx, by, seed.wrapping_add(29));
                        let bch = weighted_char(reach * 0.7, REACH_CHARS, bh);
                        fill_blend(&mut grid, bx, by, bch, Some(Layer::Reach), rgb(c_reach));
                    }
                    bt += 0.7;
                }
            }
            t += 0.55;
        }
    }

    // Macro tendrils: sparser echoes beyond main arms (fractal macro scale).
    if reach > 0.45 {
        let macro_count = ((arm_count as f32 * 0.6).round() as usize).max(0);
        let macro_len = arm_len * 1.8;
        for i in 0..macro_count {
            let a = (i as f32 / macro_count.max(1) as f32) * std::f32::consts::TAU
                + std::f32::consts::PI / macro_count.max(1) as f32
                + 0.15;
            let mut t = arm_len * 0.9;
            while t < macro_len {
                let x = (cx + a.cos() * t / 0.58).round() as i32;
                let y = (cy + a.sin() * t * 0.9).round() as i32;
                let h = cell_hash(x, y, seed.wrapping_add(41));
                if h < 0.45 + reach * 0.35 {
                    let v = reach * (1.0 - (t - arm_len) / (macro_len - arm_len) * 0.8);
                    let ch = weighted_char(v, REACH_CHARS, h);
                    fill_blend(&mut grid, x, y, ch, Some(Layer::Reach), rgb(c_reach));
                }
                t += 0.85;
            }
        }
    }

    // ---- FRACTAL MACRO MINI-MANDALAS ----
    // When reach is high, anchor a scaled-down mandala (tiny core blob + one
    // tiny ring) at evenly-spaced points along an inner ring of the ellipse.
    // Each sub-mandala uses an offset seed so its texture is distinct yet
    // deterministic. Clipped to the parent ellipse so it never paints corners.
    // See docs/specs/glyph-expansion.md (Design B).
    if reach > 0.6 {
        // Sub-mandala count grows with reach: 4 at reach=0.6 → 10 at reach=1.0.
        let sub_count = ((4.0 + (reach - 0.6) * 15.0).round() as usize).clamp(4, 10);
        // Anchor radius (ellipse-relative) — well inside so sub-mandalas don't
        // poke through the boundary at high `sub_max_r`.
        let sub_re = 0.66_f32;
        // Sub-mandala scale grows mildly with reach.
        let sub_scale = scale * (0.30 + (reach - 0.6) * 0.50);
        let sub_core_r = (sub_scale * 0.85).max(1.0);
        let sub_ring_r = sub_scale * 1.9;
        let sub_ring_w = (sub_scale * 0.55).max(0.6);
        let sub_max_r = sub_ring_r + sub_ring_w + 0.5;

        for i in 0..sub_count {
            // Place between primary arms (half-step offset) so sub-mandalas
            // don't compete visually with the main reach tendrils.
            let a = (i as f32 / sub_count as f32) * std::f32::consts::TAU
                + std::f32::consts::PI / sub_count as f32;
            let sub_x = cx + a.cos() * sub_re * cx;
            let sub_y = cy + a.sin() * sub_re * cy;
            let sub_seed = seed.wrapping_add(900).wrapping_add(i as u64 * 53);

            let ax = sub_x.round() as i32;
            let ay = sub_y.round() as i32;
            let bb = sub_max_r.ceil() as i32;
            for oy in -bb..=bb {
                for ox in -bb..=bb {
                    let lx = ax + ox;
                    let ly = ay + oy;
                    if lx < 0 || ly < 0 {
                        continue;
                    }
                    let (ux, uy) = (lx as usize, ly as usize);
                    if ux >= gw || uy >= gh {
                        continue;
                    }
                    // Local distance from sub-mandala center (aspect-corrected).
                    let ldx = (lx as f32 - sub_x) * 0.58;
                    let ldy = ly as f32 - sub_y;
                    let ld = (ldx * ldx + ldy * ldy).sqrt();
                    if ld > sub_max_r {
                        continue;
                    }
                    // Clip to parent ellipse so sub-mandalas never paint corners.
                    let gdx = (lx as f32 - cx) * 0.58;
                    let gdy = ly as f32 - cy;
                    if r_ellipse(gdx, gdy) >= 0.98 {
                        continue;
                    }
                    let h = cell_hash(lx, ly, sub_seed);

                    if ld < sub_core_r {
                        // Sub-core: dense braille cluster.
                        let v = reach * (1.0 - ld / sub_core_r * 0.35);
                        let ch = weighted_char(v, CORE_CHARS, h);
                        put_blend(&mut grid, lx, ly, ch, Some(Layer::Reach), rgb(c_reach));
                    } else if ld > sub_ring_r && ld < sub_ring_r + sub_ring_w && h < 0.65 {
                        // Sub-ring.
                        let v = reach * 0.75;
                        let ch = weighted_char(v, RING_CHARS, h);
                        put_blend(&mut grid, lx, ly, ch, Some(Layer::Reach), rgb(c_reach));
                    }
                }
            }
        }
    }

    // ---- SPINE ----
    if spine > 0.25 {
        let spine_len = (spine * (gh as f32 * 0.55)).round() as i32;
        for i in -spine_len..=spine_len {
            let y = (cy + i as f32 * 0.9).round() as i32;
            let x = cx.round() as i32;
            if (i as f32).abs() < core_radius - 0.5 {
                continue;
            }
            let h = cell_hash(x, y, seed.wrapping_add(3));
            if h < 0.65 + spine * 0.3 {
                let v = spine * (1.0 - (i as f32).abs() / spine_len as f32 * 0.5).max(0.4);
                let ch = weighted_char(v, SPINE_CHARS, h);
                put_blend(&mut grid, x, y, ch, Some(Layer::Spine), rgb(c_spine));
            }
        }
    }

    // ---- FLOW: rivers, not ocean ─────────────────────────────────────────
    // Flow renders as defined wave channels rather than pervasive fill. The
    // wave gate is tight (only the peaks pass) and the per-cell density is
    // sparse, giving a few winding bands of '~/-\=' across the field instead
    // of saturating it. Gated above 0.35 so flow doesn't overwhelm low-level
    // states. Clipped to the elliptical envelope so corners stay reserved
    // for prestige integration.
    if flow > 0.35 {
        let flow_eff = flow - 0.35; // 0..0.65 over level ~35..99
        for y in 0..gh {
            for x in 0..gw {
                let dx = (x as f32 - cx) * 0.58;
                let dy = y as f32 - cy;
                let d = (dx * dx + dy * dy).sqrt();
                if d < 2.0 {
                    continue;
                }
                // Higher wave frequency → narrower channels per cycle.
                let wave =
                    ((x as f32 - cx) * 0.65 + (y as f32 - cy) * 0.85 + seed as f32 * 0.1).sin();
                // Tight threshold: only the wave peaks pass.
                // At flow=0.35: threshold≈0.99 (top 9%).
                // At flow=1.0: threshold≈0.78 (top ~36%).
                let wave_threshold = 1.0 - flow_eff * 0.34;
                if wave.abs() > wave_threshold {
                    let h = cell_hash(x as i32, y as i32, seed.wrapping_add(11));
                    let m = ellipse_mask(dx, dy);
                    // Sparse density inside the channels.
                    if grid[y][x].ch == ' ' && h < (0.18 + flow_eff * 0.22) * m {
                        let v = flow * (wave.abs() * 0.85 + 0.15);
                        let ch = weighted_char(v, FLOW_CHARS, h);
                        put_blend(
                            &mut grid,
                            x as i32,
                            y as i32,
                            ch,
                            Some(Layer::Flow),
                            rgb(c_flow),
                        );
                    }
                }
            }
        }
    }

    // Compute last ring outer radius (needed by space, depth, warmth).
    let last_ring_r2 = if ring_count > 0 {
        let last_r = ring_count - 1;
        (3.2 + clarity * 1.4 + last_r as f32 * 2.2) * scale + 1.1 * scale
    } else {
        (3.2 + clarity * 1.4) * scale + 1.1 * scale
    };

    // ---- SPACE: punch/scatter ----
    // High space (>0.5): punch (erase) cells beyond the outer clarity ring.
    // Also scatter SPACE_CHARS in the outer quarter of the grid (d > max_r*0.5)
    // so that the Space layer always has visible attribution even when the ring
    // formula places last_ring_r2 beyond the visible grid extents.
    if space > 0.5 {
        for y in 0..gh {
            for x in 0..gw {
                let dx = (x as f32 - cx) * 0.58;
                let dy = y as f32 - cy;
                let d = (dx * dx + dy * dy).sqrt();
                if d > last_ring_r2 {
                    let h = cell_hash(x as i32, y as i32, seed.wrapping_add(19));
                    if h < (space - 0.4) * 0.6 {
                        // Space-punch: erase cell and mark it as a Space-layer cell
                        // so the layer attribution is preserved for display/testing.
                        grid[y][x] = GridCell {
                            ch: ' ',
                            layer: Some(Layer::Space),
                            blend_r: 0.0,
                            blend_g: 0.0,
                            blend_b: 0.0,
                            blend_w: 0.0,
                        };
                    }
                }
                // Secondary scatter: outer region of the ellipse, giving Space
                // cells regardless of ring geometry. Uses put_blend to guarantee
                // layer attribution; character is picked from the non-space tail
                // of SPACE_CHARS so put_blend never skips.
                if r_ellipse(dx, dy) > 0.48 {
                    let h = cell_hash(x as i32, y as i32, seed.wrapping_add(37));
                    let m = ellipse_mask(dx, dy);
                    // Space chars are wispy (dots, spaces) — density stays high so
                    // vanilla zenith reads as a misty field, not a solid block fill.
                    if h < (space - 0.3) * 0.22 * m {
                        // Force index into the non-space range (indices 2,4,6 = '.',':','.').
                        let palette_idx = if h < 0.33 {
                            2usize
                        } else if h < 0.66 {
                            4
                        } else {
                            6
                        };
                        let ch = SPACE_CHARS[palette_idx];
                        put_blend(
                            &mut grid,
                            x as i32,
                            y as i32,
                            ch,
                            Some(Layer::Space),
                            rgb(c_space),
                        );
                    }
                }
            }
        }
    } else if space < 0.35 {
        for y in 0..gh {
            for x in 0..gw {
                let dx = (x as f32 - cx) * 0.58;
                let dy = y as f32 - cy;
                let d = (dx * dx + dy * dy).sqrt();
                if d > last_ring_r2 + 0.5 && d < last_ring_r2 + 3.0 && grid[y][x].ch == ' ' {
                    let h = cell_hash(x as i32, y as i32, seed.wrapping_add(31));
                    if h < 0.1 {
                        let ch = weighted_char(0.35, SPACE_CHARS, h);
                        put_blend(
                            &mut grid,
                            x as i32,
                            y as i32,
                            ch,
                            Some(Layer::Space),
                            rgb(c_space),
                        );
                    }
                }
            }
        }
    }

    // ---- DEPTH: multiple arc bands, extending far ----
    // The primary bands may fall outside the visible grid at high clarity/small targets.
    // A secondary grid-bounded scatter ensures Depth always has visible cells.
    if depth > 0.1 {
        let bands = (depth * 10.0).round() as usize;
        for b in 0..bands {
            let base_a = (b as f32 / bands as f32) * std::f32::consts::TAU + 0.3;
            let r = last_ring_r2 + (1.2 + b as f32 * 0.9) * scale;
            let span = std::f32::consts::PI * (0.3 + depth * 0.7);
            let mut a = base_a - span / 2.0;
            while a < base_a + span / 2.0 {
                let x = (cx + a.cos() * r / 0.58).round() as i32;
                let y = (cy + a.sin() * r * 0.9).round() as i32;
                let h = cell_hash(x, y, seed.wrapping_add(43).wrapping_add(b as u64));
                let v = depth * (1.0 - b as f32 / bands as f32 * 0.6).max(0.3);
                let ch = weighted_char(v, DEPTH_CHARS, h);
                fill_blend(&mut grid, x, y, ch, Some(Layer::Depth), rgb(c_depth));
                a += 0.09;
            }
        }
        // Secondary: scatter in the outer grid ring to ensure visibility
        // when the primary arcs fall entirely outside the visible grid.
        for y in 0..gh {
            for x in 0..gw {
                let dx = (x as f32 - cx) * 0.58;
                let dy = y as f32 - cy;
                if r_ellipse(dx, dy) > 0.52 {
                    let h = cell_hash(x as i32, y as i32, seed.wrapping_add(47));
                    let m = ellipse_mask(dx, dy);
                    // Depth chars are wispy (':', ';', ',') — keep density high
                    // for ambient texture in the outer band.
                    if h < depth * 0.13 * m {
                        let ch = weighted_char(depth * 0.7, DEPTH_CHARS, h);
                        put_blend(
                            &mut grid,
                            x as i32,
                            y as i32,
                            ch,
                            Some(Layer::Depth),
                            rgb(c_depth),
                        );
                    }
                }
            }
        }
    }

    // ---- RESONANCE: sparks, far-field when high ----
    if resonance > 0.1 {
        // Inner cutoff still in absolute cells (so the core stays clear).
        let inner_r = 2.0_f32;
        // Outer extent in elliptical-normalised radius. At zenith the cap
        // expands past 1.0 so sparks scatter into the corners too.
        let outer_re = 0.4 + resonance * 0.6 + zenith_blend * 0.5;
        for y in 0..gh {
            for x in 0..gw {
                let dx = (x as f32 - cx) * 0.58;
                let dy = y as f32 - cy;
                let d = (dx * dx + dy * dy).sqrt();
                let re = r_ellipse(dx, dy);
                if d > inner_r && re < outer_re {
                    let h = cell_hash(x as i32, y as i32, seed.wrapping_add(53));
                    let m = ellipse_mask(dx, dy);
                    // Sparks are pure wispy texture — RES_CHARS is dots/asterisks.
                    // Bumped at vanilla zenith to fill the field with more wisps.
                    if h < resonance * 0.26 * m && grid[y][x].ch == ' ' {
                        let v = resonance * (1.0 - (re / outer_re) * 0.4);
                        let ch = weighted_char(v, RES_CHARS, h);
                        put_blend(
                            &mut grid,
                            x as i32,
                            y as i32,
                            ch,
                            Some(Layer::Resonance),
                            rgb(c_resonance),
                        );
                    }
                }
            }
        }
    }

    // ---- WARMTH: petals at multiple radii ----
    // Primary petals may fall outside the visible grid at high clarity/small targets.
    // A secondary grid-bounded scatter ensures Warmth always has visible cells.
    if warmth > 0.1 {
        let petal_layers = (warmth * 5.0).round().max(1.0) as usize;
        for pl in 0..petal_layers {
            let petals = ((6.0 + warmth * 16.0).round() as usize) + pl * 2;
            let petal_r = last_ring_r2 + (0.6 + pl as f32 * 2.4) * scale;
            let petal_len = (1.0 + warmth * 3.0) * scale;
            for i in 0..petals {
                let a = (i as f32 / petals as f32) * std::f32::consts::TAU
                    + std::f32::consts::PI / petals as f32
                    + 0.2
                    + pl as f32 * 0.15;
                let mut t = 0.0_f32;
                while t < petal_len {
                    let rr = petal_r + t;
                    let x = (cx + a.cos() * rr / 0.58).round() as i32;
                    let y = (cy + a.sin() * rr * 0.9).round() as i32;
                    let h = cell_hash(x, y, seed.wrapping_add(67).wrapping_add(pl as u64));
                    let v = warmth * (0.4 + (t / petal_len) * 0.6);
                    let ch = weighted_char(v, WARM_CHARS, h);
                    fill_blend(&mut grid, x, y, ch, Some(Layer::Warmth), rgb(c_warmth));
                    t += 0.55;
                }
            }
        }
        // Secondary: petal fringe parameterised in ellipse-normalised radius
        // (re ∈ 0.50..0.85, extending toward the corners at zenith). Density
        // falls off via (1 - re²) lerped toward 1 as zenith_blend grows so the
        // fringe fades smoothly mid-progress and fills aggressively at 99.
        let warm_petals = ((6.0 + warmth * 16.0).round() as usize).max(1);
        let re_cap = 0.85 + zenith_blend * 0.55;
        for i in 0..warm_petals {
            let a = (i as f32 / warm_petals as f32) * std::f32::consts::TAU + 0.1;
            let mut re = 0.50_f32;
            while re < re_cap {
                let x = (cx + a.cos() * re * cx).round() as i32;
                let y = (cy + a.sin() * re * cy).round() as i32;
                if x < 0 || y < 0 || (x as usize) >= gw || (y as usize) >= gh {
                    re += 0.06;
                    continue;
                }
                let h = cell_hash(x, y, seed.wrapping_add(71));
                let base = (1.0 - re * re).max(0.0);
                let m = (base + zenith_blend * (1.0 - base)).min(1.0);
                // Warm petals trimmed and char-selection capped low — WARM_CHARS
                // is mostly blocks (▖▗▘▝▙▚▛...). At v=0.35 we sit on quarter-blocks
                // ('▖▗▘▝') which read as petal flecks, not solid panels.
                if h < warmth * 0.18 * m {
                    let ch = weighted_char(warmth * 0.35, WARM_CHARS, h);
                    put_blend(&mut grid, x, y, ch, Some(Layer::Warmth), rgb(c_warmth));
                }
                re += 0.06;
            }
        }
    }

    // ---- AURA: far-field bloom (unlocked above progress 0.4) ----
    if progress > 0.4 {
        let aura_strength = ((progress - 0.4) * 1.8).min(1.0);
        for y in 0..gh {
            for x in 0..gw {
                let dx = (x as f32 - cx) * 0.58;
                let dy = y as f32 - cy;
                let re = r_ellipse(dx, dy);
                if re > 0.5 && re < aura_re_max {
                    // Two-peak aura at ~70% and ~95% of the ellipse radius.
                    // At zenith the cap (aura_re_max) extends past 1.0 so the
                    // bloom reaches into the rectangular corners.
                    let peak1 = 1.0 - (re - 0.70).abs() * 2.5;
                    let peak2 = 1.0 - (re - 0.95).abs() * 3.5;
                    let density = aura_strength * peak1.max(peak2 * 0.7).max(0.0);
                    if density > 0.0 {
                        let h = cell_hash(x as i32, y as i32, seed.wrapping_add(91));
                        let m = ellipse_mask(dx, dy);
                        // Aura is the blockiest outer pass — trimmed for vanilla
                        // zenith. Char selection capped to wispy palette indices
                        // (v * 0.55) so blocks ('▄', '▌', '▐') stay rare; integration
                        // unlocks the dense block end.
                        if h < density * 0.28 * m {
                            let v = (aura_strength * density * 0.55).min(0.6);
                            let ch = weighted_char(v, AURA_CHARS, h);
                            // Outer fringe (re > 0.82) overwrites the cell so Aura
                            // always attributes — those cells lie past the dense
                            // ring band so ASCII overwrites don't damage detail.
                            // Inner band uses fill_blend to preserve mandala chars.
                            if re > 0.82 {
                                put_blend(
                                    &mut grid,
                                    x as i32,
                                    y as i32,
                                    ch,
                                    Some(Layer::Aura),
                                    rgb(c_aura),
                                );
                            } else {
                                fill_blend(
                                    &mut grid,
                                    x as i32,
                                    y as i32,
                                    ch,
                                    Some(Layer::Aura),
                                    rgb(c_aura),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // ---- HALO: outer ring (unlocked at progress > 0.7) ----
    // Primary: arc-based ring at max_r fractions (may fall outside small grids).
    // Secondary: grid-bounded scatter at d > max_r*0.45 ensures Halo always paints.
    // Both use put_blend so the Halo layer always has attributed cells.
    if progress > 0.7 {
        let halo_strength = ((progress - 0.7) * 3.0).min(1.0);
        for (r_frac, density) in [(0.88_f32, 1.0_f32), (0.98, 0.55)] {
            let halo_r = max_r * r_frac;
            let mut a = 0.0_f32;
            while a < std::f32::consts::TAU {
                let x = (cx + a.cos() * halo_r / 0.58).round() as i32;
                let y = (cy + a.sin() * halo_r * 0.9).round() as i32;
                let h = cell_hash(x, y, seed.wrapping_add(113));
                if h < (0.4 + halo_strength * 0.4) * density {
                    let ch = weighted_char(halo_strength, HALO_CHARS, h);
                    put_blend(&mut grid, x, y, ch, Some(Layer::Halo), rgb(c_halo));
                }
                a += 0.022;
            }
        }
        // Secondary halo ring: grid-bounded using per-cell distance check.
        // Activates in the outer fringe where the primary ring may miss the grid.
        for y in 0..gh {
            for x in 0..gw {
                let dx = (x as f32 - cx) * 0.58;
                let dy = y as f32 - cy;
                // Target cells in the outer half of the ellipse.
                if r_ellipse(dx, dy) > 0.45 {
                    let h = cell_hash(x as i32, y as i32, seed.wrapping_add(117));
                    let m = ellipse_mask(dx, dy);
                    if h < halo_strength * 0.11 * m {
                        let ch = weighted_char(halo_strength, HALO_CHARS, h);
                        put_blend(
                            &mut grid,
                            x as i32,
                            y as i32,
                            ch,
                            Some(Layer::Halo),
                            rgb(c_halo),
                        );
                    }
                }
            }
        }
    }

    // ---- Symmetry mirroring: horizontal then vertical ----
    // A cell is "empty" iff ch==' ' AND layer is None (no layer claimed it).
    // Space-punched cells have ch==' ' but layer=Some(Space) — they are treated
    // as non-empty so the space-punch mirrors symmetrically.
    // Left wins for H-mirror; top wins for V-mirror.
    #[inline]
    fn is_empty(cell: &GridCell) -> bool {
        cell.ch == ' ' && cell.layer.is_none()
    }

    for row in grid.iter_mut() {
        for x in 0..(gw / 2) {
            let mx = gw - 1 - x;
            let left_empty = is_empty(&row[x]);
            let right_empty = is_empty(&row[mx]);
            if left_empty && !right_empty {
                row[x] = row[mx].clone();
            } else if !left_empty {
                // Left wins: copy to right unconditionally.
                row[mx] = row[x].clone();
            }
        }
    }
    // Vertical: top wins.
    // Must use index-based access since we need two distinct row indices simultaneously.
    #[allow(clippy::needless_range_loop)]
    for x in 0..gw {
        for y in 0..(gh / 2) {
            let my = gh - 1 - y;
            let top_empty = is_empty(&grid[y][x]);
            let bot_empty = is_empty(&grid[my][x]);
            if top_empty && !bot_empty {
                let src = grid[my][x].clone();
                grid[y][x] = src;
            } else if !top_empty {
                // Top wins: copy to bottom unconditionally.
                let src = grid[y][x].clone();
                grid[my][x] = src;
            }
        }
    }

    // ---- Materialise GlyphFrame ----
    let cells: Vec<Vec<GlyphCell>> = grid
        .iter()
        .map(|row| {
            row.iter()
                .map(|c| {
                    // Use blended color if available, else fall back to layer_color.
                    let fg = if c.blend_w > 0.0 {
                        Color::Rgb(
                            c.blend_r.round() as u8,
                            c.blend_g.round() as u8,
                            c.blend_b.round() as u8,
                        )
                    } else {
                        Color::DarkGray
                    };
                    GlyphCell {
                        ch: c.ch,
                        layer: c.layer,
                        fg,
                        intensity: intensity_idx,
                    }
                })
                .collect()
        })
        .collect();

    GlyphFrame {
        cells,
        cols: grid_w,
        rows: grid_h,
        intensity_idx,
        zenith,
        progress,
    }
}

// ---------------------------------------------------------------------------
// Ratatui integration
// ---------------------------------------------------------------------------

/// Write a `GlyphFrame` into a ratatui `Buffer`, centered in `area`.
/// Cells outside the glyph bounds are not touched.
/// Cells with `intensity >= 4` are rendered BOLD.
pub fn apply_to_buf(frame: &GlyphFrame, buf: &mut Buffer, area: Rect) {
    if frame.cols == 0 || frame.rows == 0 {
        return;
    }

    // Centre the glyph in the given area.
    let offset_x = (area.width.saturating_sub(frame.cols)) / 2;
    let offset_y = (area.height.saturating_sub(frame.rows)) / 2;

    // Pre-compute clip bounds with saturating_add to prevent u16 overflow panics.
    let clip_x = area.x.saturating_add(area.width);
    let clip_y = area.y.saturating_add(area.height);

    for (row_idx, row) in frame.cells.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            let buf_x = area
                .x
                .saturating_add(offset_x)
                .saturating_add(col_idx as u16);
            let buf_y = area
                .y
                .saturating_add(offset_y)
                .saturating_add(row_idx as u16);
            if buf_x >= clip_x || buf_y >= clip_y {
                continue;
            }
            if let Some(buf_cell) = buf.cell_mut((buf_x, buf_y)) {
                buf_cell.set_char(cell.ch);
                buf_cell.set_fg(cell.fg);
                if cell.intensity >= 4 {
                    buf_cell
                        .set_style(ratatui::style::Style::default().add_modifier(Modifier::BOLD));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public color helper for sidebar use.
// ---------------------------------------------------------------------------

/// Return the mandala color for a trait id at a given level progress.
///
/// `trait_id` is one of the 9 catalog trait ids (e.g. "flow", "core").
/// `norm` is 0.0..=1.0 level progress within the current level.
/// Unknown trait ids fall back to the Aura color.
pub fn trait_color(trait_id: &str, norm: f32) -> Color {
    let layer = match trait_id {
        TRAIT_FLOW => Layer::Flow,
        TRAIT_CORE => Layer::Core,
        TRAIT_SPINE => Layer::Spine,
        TRAIT_REACH => Layer::Reach,
        TRAIT_CLARITY => Layer::Clarity,
        TRAIT_SPACE => Layer::Space,
        TRAIT_DEPTH => Layer::Depth,
        TRAIT_RESONANCE => Layer::Resonance,
        TRAIT_WARMTH => Layer::Warmth,
        _ => Layer::Aura,
    };
    // Use intensity 3 (mid-range) so sidebar colors look consistent.
    layer_color(layer, norm.clamp(0.0, 1.0), 3)
}

// ---------------------------------------------------------------------------
// Exported helper: weighted_char is part of the public API per the spec.
// ---------------------------------------------------------------------------

/// Deterministically sample a char from `palette` based on trait value `v`
/// and a hash `h` (both in [0.0, 1.0)). Provides textural variety: at v=0.9
/// you still occasionally see mid-density chars. See `weightedChar` in glyph.jsx.
pub fn weighted_char_pub(v: f32, palette: &[char], hash: u64) -> char {
    // Convert u64 hash to [0, 1) float.
    let h = (hash % 10_000) as f32 / 10_000.0;
    weighted_char(v, palette, h)
}
