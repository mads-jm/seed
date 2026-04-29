---
tags:
  - index
date created: Wednesday, April 29th 2026, 7:41:45 am
date modified: Wednesday, April 29th 2026, 7:53:26 am
---

# Concepts

Atomic concept notes — durable, reusable knowledge about patterns and primitives that recur across `seed`. Each note is self-contained and linkable: when a spec or build-log mentions one of these patterns, it wikilinks here rather than re-explaining.

[[CONCEPTS.base]]

## Planned (referenced by Existing Prose, not yet written)

These wikilinks already appear across the vault as unresolved nodes in the graph. They mark patterns the existing docs lean on implicitly; the next concepts pass writes the canonical definitions.

- [[event-sourcing]] — fold-based state, append-only event log, replay determinism. The single invariant that `apply_event` is the only state mutation, used by both daemon (writer) and TUI (reader).
- [[pure-core]] — the no-I/O invariant in `seed-core`. Clock injected as `now: DateTime<Utc>`; paths injected as `&Path`. Why golden tests + replay work.
- [[wire-versioning]] — the `{ v, ts, kind, data }` envelope; additive (new kind / new field with default) vs breaking (rename / retype) classification; `Event::Unknown` for forward-compat round-trip.
- [[reminder-lifecycle]] — the `Off / Dormant / Due / Overdue` state machine, the 1.0×I → 1.5×I → 2.0×I thresholds, and the auto-skip rollover that bounds the Overdue window.
- [[tier-progression]] — 10 tiers from `total_level` (sum of all 9 trait levels), `tier_for()`. Why integrate steps tier down (intentional).
- [[glyph-layer-composition]] — 11 layers, symmetry mirror, blend accumulators. How `prestige-integrate` enhancements stack additively on the per-trait layer contribution.
- [[snapshot-and-replay]] — daemon startup pattern (load snapshot → fold tail past `skip_count`), periodic snapshot every 100 events / 5 min, why `apply_event` purity makes this safe.
- [[terminal-capability-ladder]] — truecolor → 256-cube → 16-color; braille → block-density via popcount. Env-driven probes (`SEED_FORCE_ASCII`, `SEED_FORCE_256`) as escape hatches.

When writing each note: lead with the invariant or rule the concept enforces, then a small example, then a "where it shows up" list of links back to the specs / build-logs that depend on it.

