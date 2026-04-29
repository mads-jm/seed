---
date created: Tuesday, April 28th 2026, 12:00:00 pm
date modified: Wednesday, April 29th 2026, 7:53:25 am
cssclasses: []
tags:
  - spec
  - glyph
  - rendering
  - orbit
status: implemented
---

# Glyph Expansion & Orbit Layout

Status: A, B, C all implemented. Vanilla zenith intentionally tuned at ~50%
visual headroom to leave room for future prestige / integration layers.

## Prestige / Integration Headroom

Future "integration" unlocks (earned by resetting individual traits from
99 back to 1) will introduce __solid block forms__ — the true core of the
companion — layered on top of the textured base mandala. The current
renderer therefore tunes vanilla level 99 across all 9 traits as a
__wispy, complete-but-not-overwhelming__ field rather than a saturated
block fill. Concretely:

- `ZENITH_CAP = 0.5` — the smoothstep zenith blend caps at 0.5, so the
  elliptical mask only lerps halfway to 1.0 at vanilla zenith. Corners
  paint at half density; integration unlocks lifting the cap toward 1.0.
- Outer scatter densities trimmed (Aura ×0.28, Halo-secondary ×0.11,
  Warmth-secondary ×0.18, …).
- Char-selection capped at the wispy end of palettes (`v * 0.55` for
  Aura, `v * 0.35` for Warmth-secondary) so block characters
  ('▄', '▌', '▐', '█') stay rare in vanilla. Integration relaxes the cap
  to introduce solid forms.
- Resonance density bumped (×0.26) and Depth-secondary density kept
  generous (×0.13) — these layers use wispy chars (sparks, dots,
  punctuation) and are the primary "wisp" providers.
- __Flow__ is reworked as rivers, not ocean (see Design A below).

## Problem

The orbit pane renders a rectangular glyph silhouette regardless of trait
combination. At mid-to-high progress, the outer scatter passes (Aura, Halo
secondary, Warmth secondary, Depth secondary, Resonance) paint corner cells
densely because their gating uses `d > max_r * X` where `max_r` is the corner
distance — every cell in the bounding box at sufficient `d` qualifies. The
backlog explicitly asks for "fractal influences both micro and macro" and "99
in all skills should be visually intense"; the current output reads as a
rectangle of texture, not a mandala.

A second issue: orbit reminder cards float on a calculated ellipse and are
filtered by a clipping check. On smaller terminals or off-axis card positions
they get dropped silently, so the pane has cards appearing/disappearing as the
window resizes.

## Goals

1. Glyph silhouette is __round__ (or organically irregular), not rectangular.
   Empty space at the corners is intentional and reserved for cards/menus.
2. Orbit cards live in __fixed bounded boxes__ that are guaranteed to render.
   No silent clipping. Card visibility never depends on glyph geometry.
3. Visual progression remains exponential: at zenith the glyph fills its
   elliptical envelope densely; at low progress it stays sparse.
4. Render path remains pure / deterministic / cheap — see [[pure-core]] and the [[glyph-layer-composition]] invariants.

## Non-goals

- Animating the silhouette shape (the envelope is fixed per frame).
- Replacing the ratatui Buffer rendering pipeline.
- Per-trait silhouette morphing (a future possibility, not this spec).

---

## Design

### A. Elliptical Falloff Mask + Zenith Expansion (IMPLEMENTED)

The outer scatter layers are gated by a soft elliptical mask centred on the
glyph grid. Cells outside the ellipse get zero density; cells near the
ellipse boundary fade smoothly; cells deep inside paint at full density.

Mask formula, evaluated per cell using the same aspect-corrected coords the
glyph already uses (`dx = (x - cx) * 0.58`, `dy = y - cy`):

```
dx_max       = cx * 0.58
dy_max       = cy
r_norm²      = (dx / dx_max)² + (dy / dy_max)²
base         = max(0, 1 − r_norm²)         // 1 at center, 0 at ellipse boundary
zenith_blend = smoothstep((progress − 0.85) / 0.15, 0..1)
mask         = min(1, base + zenith_blend · (1 − base))
```

`zenith_blend` ramps from 0 below `progress = 0.85` to 1 at `progress = 1.0`
(smoothstep). At low/mid progress `mask = base` (full elliptical falloff —
corners empty, reserved for cards). At zenith `mask = 1` everywhere — outer
scatter passes paint the rectangular corners at full density and the glyph
fills the entire pane behind the cards.

Aura's explicit upper-radius cap follows the same lerp:

```
aura_re_max = 1.0 + zenith_blend · 0.55
```

With `ZENITH_CAP = 0.5` this gives `aura_re_max ≈ 1.275` at vanilla zenith
— enough to push past the ellipse but not all the way to the corners.

Resonance and Warmth-secondary upper caps relax in the same way.

### Flow (rivers, not ocean)

Flow is the most-visible textural layer and was originally tuned too
aggressively. Reworked:

- Gate: `flow > 0.35` (no flow until level ~35) — was `flow > 0.2`.
- Wave frequency bumped (`* 0.65, * 0.85`) — narrower channels per cycle.
- Tight wave threshold: `1.0 - flow_eff * 0.34` where
  `flow_eff = flow - 0.35`. At max flow only the top ~36% of wave heights
  pass; at gate-clearing flow only the top ~9% pass. Result: thin defined
  channels rather than parallel bands of fill.
- Sparse density: `0.18 + flow_eff * 0.22`.
- Multiplied by `ellipse_mask` so flow stays inside the envelope —
  prestige integration reserves the corner space for solid forms.

`mask` multiplies the density threshold for the following layers / passes:

| Layer / Pass                                        | Effect                                            |
|-----------------------------------------------------|---------------------------------------------------|
| Aura primary (`nr > 0.5`)                           | density `× mask`                                  |
| Halo secondary (`d > max_r * 0.45`)                 | density `× mask`                                  |
| Warmth secondary (outer petal fringe)               | density `× mask`                                  |
| Depth secondary (outer scatter)                     | density `× mask`                                  |
| Resonance scatter                                   | density `× mask`                                  |
| Space secondary scatter (when `space > 0.5`)        | density `× mask`                                  |

Structural inner layers (Core, Clarity rings, Reach arms+branches, Spine,
Flow diagonals) are __not__ masked — they're already inherently radial /
angular and produce no rectangular fill on their own. The petal / band
*primary* passes (Warmth primary, Depth primary, Halo primary) use angular
sweeps that already fall on circles; they remain unmasked too.

The `all_layers_present_at_zenith` regression test still passes because each
masked layer retains visible cells along the ellipse interior.

#### Tradeoffs

- __+__ Removes rectangular silhouette in one stroke.
- __+__ Pure additive: just multiplies an existing scalar, no new rendering
  pass.
- __−__ At very low progress, outer layers were sparse anyway so the visual
  delta is concentrated in mid-to-high progress states.
- __−__ Slightly fewer painted cells at zenith (corner cells removed).
  Mitigated by the orbit-card layout reclaiming that real estate.

### B. Recursive Macro Fractal (IMPLEMENTED)

When `reach > 0.6`, evenly-spaced anchor points along the inner ellipse
(at `r_ellipse = 0.66`, half-step rotated against the primary arms) host a
scaled-down mandala: a tiny braille core blob plus one ring drawn from
`RING_CHARS`. Each sub-mandala uses an offset seed
(`seed.wrapping_add(900 + i * 53)`) so its texture is distinct yet
deterministic.

Tunables (in `glyph.rs`):

```
sub_count    = clamp(round(4 + (reach - 0.6) * 15), 4..10)
sub_re       = 0.66                                   // anchor at 66% ellipse radius
sub_scale    = scale * (0.30 + (reach - 0.6) * 0.50)  // grows with reach
sub_core_r   = max(sub_scale * 0.85, 1.0)
sub_ring_r   = sub_scale * 1.9
sub_ring_w   = max(sub_scale * 0.55, 0.6)
sub_max_r    = sub_ring_r + sub_ring_w + 0.5
```

Anchors are placed in clockwise order at angles
`a = i / sub_count * τ + π / sub_count` so they sit between the primary
reach arms and don't visually compete with them.

Constraints:

- Each sub-mandala bounds-iterates over a `(2·bb+1)²` cell box where
  `bb = ceil(sub_max_r)` — typically 3–5 — so total work is bounded by
  `sub_count × ~100` cells (≤ 1000 cell visits per frame at zenith).
- Per-cell guard `r_ellipse(parent) < 0.98` clips sub-mandalas to the
  parent ellipse so they never paint corners.
- Sub-cells attribute to `Layer::Reach` (same layer as the parent
  tendrils) — preserves the test invariant that all layers have ≥ 1 cell
  at zenith without inventing a new layer.
- Determinism preserved via the offset seed.
- Skipped entirely when `reach ≤ 0.6` so low-/mid-progress states are
  unchanged. The `mid_traits` golden snapshot (reach=0.5) is therefore
  unaffected.

#### Tradeoffs

- __+__ True self-similarity matches the design intent literally.
- __+__ Pushes visual ambition without enlarging the silhouette beyond what
  the elliptical mask grants.
- __−__ Adds render cost proportional to `arm_count × sub_radius²` — small
  but worth measuring on small terminals.
- __−__ More tuning surface (sub_scale, density, ring count). Should ship
  with a regression test asserting "every macro tendril at high reach has
  at least N painted sub-cells within R of its tip".
- __−__ Deferred until A is in and the silhouette is right; otherwise the
  fractal detail gets buried in rectangle texture.

### C. Orbit Card Grid + Coupled Growth (IMPLEMENTED)

Replace the ellipse-floating layout with a __fixed 3×3 grid__, and let the
glyph paint the *entire pane area* with the cards floating on top in their
slots. As progress grows, the cards orbit inward and the mandala (driven by
SIZE_STEPS + the elliptical mask + zenith blend) expands to fill the freed
rim — so growth feels coupled.

```
 ┌──────────────────────────────────────┐
 │                                      │
 │   ┌────┐         TM         ┌────┐   │   ← cards inset by `inset_v`
 │   │ TL │       . . .        │ TR │   │
 │   └────┘                    └────┘   │
 │                                      │
 │   ┌────┐ glyph rim/aura/halo ┌────┐  │
 │   │ ML │ also paints OUTSIDE │ MR │  │
 │   └────┘ the orbit at zenith └────┘  │
 │                                      │
 │   ┌────┐         BM         ┌────┐   │
 │   │ BL │                    │ BR │   │
 │   └────┘                    └────┘   │
 │                                      │
 └──────────────────────────────────────┘
   ↑                                  ↑
   inset_h                        inset_h
```

- `card_w = 16`, `card_h = 3`.
- Glyph render rect = full `area` (no margin reservation). Cards stamp on
  top in their slots; the cells beneath them are wasted glyph paint but the
  cost is bounded by `8 × card_w × card_h ≈ 384` cells per frame.
- Card inset is __progress-driven__ (`progress = total_level / 891`):
  - `inset_h: 4 → 14` cells (linear)
  - `inset_v: 1 → 3` cells (linear)
- Each of the 8 conceptual positions (TM/TR/MR/BR/BM/BL/ML/TL) becomes a
  fixed top-left coordinate via the grid intersections (with the
  progress-driven inset applied at the start/end edges).
- __Card readability__ against dense glyph texture:
  - 1-cell horizontal padding (left + right only) around each card is
    `Clear`-ed to empty cells — a soft "moat" separating the card from
    glyph texture. Top/bottom butt directly against the mandala for a
    tighter vertical silhouette.
  - Card body has a solid `palette.bg2` backdrop (`Block` widget) so the
    card text never reads through to the mandala underneath. Cell `bg`
    persists when the subsequent `set_line` writes spans (spans only set
    `fg`, leaving `bg` untouched).

Slot count → position indices, walking clockwise from top-centre:

| `n` | Position indices in clockwise order      |
|-----|------------------------------------------|
| 4   | TM, MR, BM, ML                            (compass — matches old ellipse layout) |
| 6   | TM, TR, BR, BM, BL, TL                    (4 corners + top/bottom mid) |
| 8   | TM, TR, MR, BR, BM, BL, ML, TL            (full perimeter) |

`responsive_slot_count(area.width)` keeps its current thresholds (≥100 → 8,
≥70 → 6, else 4).

Cards render after the glyph, so they always overwrite glyph cells in their
slot. The elliptical mask leaves those corners empty anyway, so visually
nothing competes.

#### Tradeoffs

- __+__ Cards never disappear. No silent drops.
- __+__ Glyph gains margin: with cards at corners not orbiting, glyph_margin
  shrinks from `(18, 5)` to `(card_w, card_h) = (16, 3)`, giving the glyph
  more vertical real estate.
- __−__ Loses the "orbiting" metaphor — cards are static slots, not floating
  satellites. The original ellipse spacing read as gentle motion when slot
  counts changed. Mitigated by keeping the order-of-fill (catalog order)
  consistent.
- __−__ On very narrow widths (< ~50 cols), even fixed slots overlap. We
  fall back to slot count 4 (compass) below width 70; below ~50 the centre
  glyph area collapses but the cards still fit.

---

## Implementation History

1. Spec drafted.
2. Elliptical falloff mask in `glyph.rs` (design A) — shipped.
3. Fixed-grid card layout in `orbit.rs` (design C) — shipped, with a 6-cell
   horizontal / 1-cell vertical inset so cards sit tight around the glyph
   instead of flush against the pane frame.
4. Recursive macro fractal (design B) — shipped, gated on `reach > 0.6`.
   `mid_traits` (reach=0.5) golden snapshot remains valid; zenith
   layer-presence test still passes.

## Out of Scope (future passes)

- Per-trait silhouette warping (e.g. resonance pulls outer ring inward).
- Animation of the elliptical envelope.
- Card content layout (already fixed at icon + name + bar + verb).
- Recursive *nested* sub-mandalas (this pass implements one level of
  recursion; further levels would need their own seed offsets and an even
  tighter `sub_scale` to remain readable at terminal cell density).

## See also

- [[glyph-layer-composition]] — the underlying 11-layer structure this spec gates with the elliptical mask
- [[pure-core]] — the invariant that lets golden-snapshot tests lock the renderer
- [[prestige-integrate]] — the future "solid block forms" the wispy zenith leaves headroom for
