---
date created: Tuesday, April 28th 2026, 12:00:00 pm
date modified: Wednesday, April 29th 2026, 7:53:23 am
cssclasses: []
tags:
  - note
  - build-log
  - wave-2
status: archived
---

# Build Log 02b — Glyph Renderer

__Tasks__: TASK-004 · __Wave__: 2

## Scope

Pure renderer for the mandala glyph (the substrate of [[glyph-layer-composition]]). ~1,200 lines across `glyph.rs`, `tests/glyph.rs`, `glyph_golden.txt`. Pulled `ratatui` into `seed-core` (the only crate that needs `Buffer`) — [[pure-core]] still holds: no I/O, no clock, just `(traits, seed, target) → GlyphFrame`. Initial pass: 127/127 tests green; after Wave 2B.1 fixes, 131/131. Release perf: 1.4ms/frame at 159×79 — 11× headroom against the 16ms target.

## Technical Decisions

- __Hash function ports JSX exactly__ — `u32::wrapping_mul` mirrors JS `>>> 0` unsigned 32-bit wrap. Same constants, same distribution, same `(h % 10000) / 10000` output. Wave 2B.1 widened the seed handling to a full `u64` (splitmix-folded) after the inspector flagged a `u32` truncation collision.
- __Weighted-char distribution ports JSX exactly__ — asymmetric Gaussian-like spread, `u^1.9` center bias, 5%-chance low-dip at high `v`, 1%-chance high-poke at low `v`. This is the "even 99 has some non-max chars" texture the brief calls for.
- __Linear-RGB blend, not oklab__ — avoids a new dep. Inspector flagged this as a quality regression vs JSX's HSL-space blend (zenith mid-tones look muddier); accepted as a tradeoff. Revisit if the published mandala feels off.
- __Character palettes by layer__ — braille for inner core / clarity, box-drawing for spine, diagonals (`╱╲╳╪╫╭`) for reach, block + half-block for warmth/aura/halo. Wave 2B.1 expanded the half-block sets after the inspector found `▄▌▐▙▚▛▜▞▟` missing from emitted output.
- __Grid sizing is exponential, target-aware, always odd__ — `progress^0.6` for visible growth, `max_w * progress^0.5` to fill canvas, capped at 160×80. Odd dimensions guarantee an exact center cell.
- __Determinism__ — only `wrapping_mul`, no `RandomState` / `thread_rng` / `SystemTime`. Verified byte-equal across in-process and process-to-process runs.
- __Symmetry mirror__ — H then V, port faithful to JSX. Wave 2B (initial) wrote ghost-empty cells from `weighted_char` returning `' '`, which left the grid asymmetric where both halves were already non-space; Wave 2B.1 made the mirror unconditional and the tests now assert char-level equality.
- __Branch cap__ — JSX's `branchLen = min(t * 0.35, armLen * 0.2)` capped at 12 to bound the inner loop. Real values stay ≤ ~7 at 160×80; the cap is a safety guard for larger future canvases.

## Inspector Findings & Fixes

| Finding | Fix |
|---|---|
| __F1__ · 16 call sites used `seed + N` (naked `u64` adds). `render_glyph(_, u64::MAX, _)` panicked in debug; release silently wrapped. The docstring promised "never panics." | All sites switched to `seed.wrapping_add(N)`. New test: `no_panic_on_max_seed`. |
| __F2__ · Multi-hue layering didn't actually stack. `put` overwrote, `fill` only wrote into empty cells. At zenith, __Warmth, Depth, Halo had 0 cells__ because the inner layers had already painted everything. The "outer warm petals / violet depth / warm gold halo" from the brief were invisible. | New `GridCell` carries `blend_r/g/b/w` accumulators; `put_blend`/`fill_blend` always blend colour while `layer` tracks the dominant contributor. Secondary in-grid scatter added for Space/Depth/Warmth/Halo to bridge the geometry gap at (80,40). New test: `all_layers_present_at_zenith`. |
| __F3__ · Symmetry tests only checked `left == ' '` iff `right == ' '`. 307 of 961 horizontal pairs and 381 of 945 vertical pairs had __different non-space chars__. The AC said "Symmetry mirroring preserved" — it wasn't. | Symmetry pass became unconditional (left/top wins); root cause was empty `weighted_char` returns writing ghost state — fixed by early-returning when `ch == ' '`. Tests now assert `ch`, `layer`, and `fg` equality. |
| __F4__ · `apply_to_buf` did plain `u16` adds for `buf_x` / `buf_y`. `area.x = u16::MAX - 5, width = 10` panicked before clipping. Hostile or buggy callers crash the whole TUI. | All u16 arithmetic switched to `saturating_add`; `clip_x` / `clip_y` pre-computed once. New test: `apply_to_buf_no_panic_near_max_x`. |
| __F5__ · `f32::NAN.clamp(0, 1)` returns NaN. One NaN trait value silently disappeared arms, suppressed aura/halo, and corrupted `progress`. No panic, just invisible degradation. | Explicit `is_nan()` guard in `tv()` returns 0.0 before clamp. New test: `nan_trait_does_not_poison`. |
| __F6__ · `u64` seed truncated to `u32` before hashing. Two seeds differing only in the high 32 bits produced identical mandalas. | Full `u64` folded via splitmix: `s_lo + s_hi * 2654435761 * 2147483647`. |
| __F7__ · `▄▌▐▙▚▛▜▞▟` half-blocks were called out by AC but absent from output. | Expanded `WARM_CHARS` (18), `AURA_CHARS` (10), `HALO_CHARS` (12). Combined with the F2 layering fix, these now render. |

### Findings Carried forward

- __Linear-RGB vs oklab quality regression__ — accepted; revisit if the rendered colours feel off.
- __`golden_snapshot` line-ending fragility on Windows__ — no `.gitattributes` pin. Watch for CRLF flake on first-time Windows checkouts.
- __`pick_size` clamps below `base_w` on tiny targets__ — silent; intentional.

### Geometry Gap at (80,40) Zenith

The clarity ring formula places `last_ring_r2 ≈ 42.5` in non-square coordinates, but the maximum visual distance in the grid is only ~29.5 (0.58 x-axis compression). Space/Depth/Warmth/Halo primary rendering zones fall outside the visible grid at this scale. Wave 2B.1 added secondary grid-bounded scatter passes at `max_r * 0.45–0.68` for each. Conservative fix; primary geometry works correctly at 160×80.

## See also

- [[build-log-03-seed-daemon]] — daemon (TASK-006/007).
- [[glyph-expansion]] — later spec evolving silhouette + orbit-card layout (designs A/B/C, all shipped).

