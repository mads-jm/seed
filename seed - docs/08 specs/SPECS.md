---
tags:
  - index
date created: Wednesday, April 29th 2026, 7:01:21 am
date modified: Wednesday, April 29th 2026, 7:53:25 am
---

# Specs

Feature and component specifications.

[[SPECS.base]]

## XP, Levels, Prestige

- [[xp-pacing]] — the 1-year-to-99 contract: rescaled OSRS curve, per-reminder XP rewards, pacing bands enforced by tests
- [[prestige-integrate]] — per-trait reset to 1 with a persistent visual enhancement; cosmetic, no XP-rate change
- [[prestige-focus]] — `tokens` currency awarded every +99 cumulative levels gained; spending opens a phase that distributes a 4× XP bonus across 1–3 traits

## Scheduling / Lifecycle

- [[overdue-rollover]] — bound `Overdue` at 2×I; auto-skip restores Dormant, gate XP drain by active hours
- [[presence-grace]] — (draft, v0.2.0) injected presence signal makes absence penalty-free: away time rolls reminders forward with no streak break, no missed count, no drain

## Event Log / State

- [[replay-fidelity]] — (draft) `CompanionAwakened` is a no-op in `apply_event` while `Action::Reset` rebuilds state in memory, so a full refold across a reset silently reconstructs the wrong state; makes `snapshot.json` load-bearing rather than derived

## Rendering

- [[glyph-expansion]] — elliptical falloff mask, recursive macro fractal, fixed orbit-card grid (designs A/B/C, all shipped)

## Active Backlog

- [[v0-1-0-punch-list]] — full prose for TASK-013..028 and the v0.1.0 cut order. The [[BACKLOG.kanban|kanban board]] tracks active state; this doc is the source of truth for AC and reality notes.


