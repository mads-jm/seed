//! Integration tests for the glyph renderer (TASK-004).
//! All tests must pass with `cargo test --workspace`.

use std::collections::BTreeMap;

use pretty_assertions::assert_eq;
use seed_core::{
    domain::TraitId,
    glyph::{GlyphFrame, Layer, TraitMap, render_glyph},
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn all_traits(v: f32) -> TraitMap {
    let mut m = BTreeMap::new();
    for key in &[
        "flow",
        "core",
        "spine",
        "reach",
        "clarity",
        "space",
        "depth",
        "resonance",
        "warmth",
    ] {
        m.insert(TraitId(key.to_string()), v);
    }
    m
}

fn mid_traits() -> TraitMap {
    let mut m = BTreeMap::new();
    for key in &[
        "flow",
        "core",
        "spine",
        "reach",
        "clarity",
        "space",
        "depth",
        "resonance",
        "warmth",
    ] {
        m.insert(TraitId(key.to_string()), 0.5);
    }
    m
}

/// Serialize a frame to a string of chars (ignoring colour), row-by-row, newline-separated.
fn frame_to_char_string(frame: &GlyphFrame) -> String {
    frame
        .cells
        .iter()
        .map(|row| row.iter().map(|c| c.ch).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Count non-space cells in a frame.
fn lit_cells(frame: &GlyphFrame) -> usize {
    frame
        .cells
        .iter()
        .flat_map(|row| row.iter())
        .filter(|c| c.ch != ' ')
        .count()
}

// ---------------------------------------------------------------------------
// 1. Determinism
// ---------------------------------------------------------------------------

#[test]
fn determinism() {
    let traits = mid_traits();
    let a = render_glyph(&traits, 42, (80, 24));
    let b = render_glyph(&traits, 42, (80, 24));
    // Compare cell by cell.
    assert_eq!(a.cols, b.cols);
    assert_eq!(a.rows, b.rows);
    assert_eq!(a.cells.len(), b.cells.len());
    for (ra, rb) in a.cells.iter().zip(b.cells.iter()) {
        for (ca, cb) in ra.iter().zip(rb.iter()) {
            assert_eq!(ca, cb, "cells differ");
        }
    }
}

// ---------------------------------------------------------------------------
// 2. Symmetry
// ---------------------------------------------------------------------------

#[test]
fn horizontal_symmetry() {
    let traits = mid_traits();
    let frame = render_glyph(&traits, 7, (79, 39));
    let w = frame.cols as usize;
    for (row_idx, row) in frame.cells.iter().enumerate() {
        for x in 0..(w / 2) {
            let mx = w - 1 - x;
            let left = &row[x];
            let right = &row[mx];
            assert_eq!(
                left.ch, right.ch,
                "H-symmetry ch broken at row={row_idx} x={x} mx={mx}: '{}' vs '{}'",
                left.ch, right.ch
            );
            // fg and layer are derived from the same GridCell after the symmetry copy,
            // so they must match wherever ch matches.
            assert_eq!(
                left.layer, right.layer,
                "H-symmetry layer broken at row={row_idx} x={x} mx={mx}: {:?} vs {:?}",
                left.layer, right.layer
            );
            // Relax fg check to only when both non-space: space cells get DarkGray
            // regardless of any residual blend state from adjacent layers.
            if left.ch != ' ' {
                assert_eq!(
                    left.fg, right.fg,
                    "H-symmetry fg broken at row={row_idx} x={x} mx={mx} ch='{}': {:?} vs {:?}",
                    left.ch, left.fg, right.fg
                );
            }
        }
    }
}

#[test]
fn vertical_symmetry() {
    let traits = mid_traits();
    let frame = render_glyph(&traits, 7, (79, 39));
    let h = frame.rows as usize;
    for x in 0..frame.cols as usize {
        for y in 0..(h / 2) {
            let my = h - 1 - y;
            let top = &frame.cells[y][x];
            let bot = &frame.cells[my][x];
            assert_eq!(
                top.ch, bot.ch,
                "V-symmetry ch broken at x={x} y={y} my={my}: '{}' vs '{}'",
                top.ch, bot.ch
            );
            assert_eq!(
                top.layer, bot.layer,
                "V-symmetry layer broken at x={x} y={y} my={my}: {:?} vs {:?}",
                top.layer, bot.layer
            );
            if top.ch != ' ' {
                assert_eq!(
                    top.fg, bot.fg,
                    "V-symmetry fg broken at x={x} y={y} my={my} ch='{}': {:?} vs {:?}",
                    top.ch, top.fg, bot.fg
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Zenith flag
// ---------------------------------------------------------------------------

#[test]
fn zenith_true_when_all_max() {
    let frame = render_glyph(&all_traits(1.0), 42, (80, 40));
    assert!(frame.zenith, "expected zenith=true when all traits=1.0");
}

#[test]
fn zenith_false_when_one_below_threshold() {
    let mut traits = all_traits(1.0);
    traits.insert(TraitId("depth".to_string()), 0.96);
    let frame = render_glyph(&traits, 42, (80, 40));
    assert!(!frame.zenith, "expected zenith=false when one trait=0.96");
}

// ---------------------------------------------------------------------------
// 4. Grid scales with progress
// ---------------------------------------------------------------------------

#[test]
fn grid_scales_with_progress() {
    let empty = all_traits(0.0);
    let full = all_traits(1.0);
    let target = (80, 40);

    let low = render_glyph(&empty, 42, target);
    let high = render_glyph(&full, 42, target);

    let low_lit = lit_cells(&low);
    let high_lit = lit_cells(&high);

    assert!(
        low_lit < 200,
        "expected < 200 lit cells at progress=0, got {low_lit}"
    );
    assert!(
        high_lit > 400,
        "expected > 400 lit cells at progress=1, got {high_lit}"
    );
    assert!(
        high_lit > low_lit * 2,
        "expected full frame to have >2x the lit cells of empty frame (high={high_lit}, low={low_lit})"
    );
}

// ---------------------------------------------------------------------------
// 5. Edge cases — no panic
// ---------------------------------------------------------------------------

#[test]
fn edge_target_one_by_one() {
    let _ = render_glyph(&mid_traits(), 42, (1, 1));
}

#[test]
fn edge_target_zero_by_zero() {
    let frame = render_glyph(&mid_traits(), 42, (0, 0));
    assert_eq!(frame.cols, 0);
    assert_eq!(frame.rows, 0);
    assert!(frame.cells.is_empty());
}

#[test]
fn edge_target_large() {
    let frame = render_glyph(&all_traits(1.0), 99, (200, 100));
    // Grid is capped at 160×80 (or odd adjustment thereof).
    assert!(frame.cols <= 160, "cols={}", frame.cols);
    assert!(frame.rows <= 80, "rows={}", frame.rows);
}

#[test]
fn edge_all_traits_zero() {
    let frame = render_glyph(&all_traits(0.0), 42, (80, 40));
    assert!(!frame.zenith);
    assert!(frame.progress < 0.01);
}

#[test]
fn edge_all_traits_one() {
    let frame = render_glyph(&all_traits(1.0), 42, (80, 40));
    assert!(frame.zenith);
    assert!((frame.progress - 1.0).abs() < 0.01);
}

#[test]
fn edge_missing_trait_keys_defaults_to_zero() {
    // Empty map — all traits default to 0.0, no panic.
    let empty: TraitMap = BTreeMap::new();
    let frame = render_glyph(&empty, 42, (40, 20));
    assert!(!frame.zenith);
}

// ---------------------------------------------------------------------------
// 6. New regression tests for Wave 2B.1 inspector findings
// ---------------------------------------------------------------------------

/// Fix 1: seed = u64::MAX must never panic (wrapping_add guards).
#[test]
fn no_panic_on_max_seed() {
    let _ = render_glyph(&BTreeMap::new(), u64::MAX, (40, 20));
}

/// Fix 2: All 11 layers must have ≥ 1 cell at zenith (all traits=1.0).
#[test]
fn all_layers_present_at_zenith() {
    let frame = render_glyph(&all_traits(1.0), 42, (80, 40));
    let layers_to_check = [
        Layer::Core,
        Layer::Clarity,
        Layer::Reach,
        Layer::Spine,
        Layer::Flow,
        Layer::Space,
        Layer::Depth,
        Layer::Resonance,
        Layer::Warmth,
        Layer::Aura,
        Layer::Halo,
    ];
    for target_layer in layers_to_check {
        let count = frame
            .cells
            .iter()
            .flat_map(|row| row.iter())
            .filter(|c| c.layer == Some(target_layer))
            .count();
        assert!(
            count >= 1,
            "Layer {:?} has 0 cells at zenith — blending is broken",
            target_layer
        );
    }
}

/// Fix 4: apply_to_buf with near-MAX area.x must not panic.
#[test]
fn apply_to_buf_no_panic_near_max_x() {
    use ratatui::{buffer::Buffer, layout::Rect};
    use seed_core::glyph::apply_to_buf;

    let frame = render_glyph(&all_traits(1.0), 42, (20, 10));
    let area = Rect {
        x: u16::MAX - 10,
        y: 0,
        width: 100,
        height: 20,
    };
    // Buffer must be large enough to contain the area without wrapping.
    // Use a small buffer that doesn't actually cover the area — apply_to_buf
    // must clip cleanly and not panic when buf.cell_mut returns None.
    let mut buf = Buffer::empty(Rect {
        x: 0,
        y: 0,
        width: 20,
        height: 20,
    });
    apply_to_buf(&frame, &mut buf, area); // must not panic
}

/// Fix 5: NaN trait value must not panic or produce NaN in output.
#[test]
fn nan_trait_does_not_poison() {
    let mut traits = all_traits(0.5);
    traits.insert(TraitId("flow".to_string()), f32::NAN);
    let frame = render_glyph(&traits, 42, (40, 20));
    // Must complete without panic; progress must be finite.
    assert!(frame.progress.is_finite(), "progress is NaN/inf");
}

// ---------------------------------------------------------------------------
// 7. Performance (ignored by default; run with --ignored in release)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn perf_glyph_159x79() {
    let traits = all_traits(1.0);
    let target = (159, 79);

    // Warm up.
    let _ = render_glyph(&traits, 42, target);

    let start = std::time::Instant::now();
    for _ in 0..10 {
        let _ = render_glyph(&traits, 42, target);
    }
    let elapsed = start.elapsed();
    let per_frame_ms = elapsed.as_secs_f64() * 1000.0 / 10.0;

    println!("perf_glyph_159x79: {per_frame_ms:.2}ms/frame");
    assert!(
        per_frame_ms < 16.0,
        "render at 159×79 took {per_frame_ms:.2}ms — must be <16ms in release"
    );
}

// ---------------------------------------------------------------------------
// 7. Golden snapshot — locks visual output schema.
// Derived from: traits all=0.5, seed=42, target=(40,20).
// Run `cargo test dump_golden -- --nocapture` once to capture the initial snapshot.
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn dump_golden() {
    let traits = mid_traits();
    let frame = render_glyph(&traits, 42, (40, 20));
    println!("cols={} rows={}", frame.cols, frame.rows);
    println!("--- BEGIN SNAPSHOT ---");
    println!("{}", frame_to_char_string(&frame));
    println!("--- END SNAPSHOT ---");
}

/// The golden snapshot — byte-equal expected output for mid_traits, seed=42, target=(40,20).
/// If you change the renderer in a way that intentionally alters output, update this string.
const GOLDEN_SNAPSHOT: &str = include_str!("glyph_golden.txt");

#[test]
fn golden_snapshot() {
    let traits = mid_traits();
    let frame = render_glyph(&traits, 42, (40, 20));
    let actual = frame_to_char_string(&frame);
    assert_eq!(actual, GOLDEN_SNAPSHOT.trim_end_matches('\n'));
}
