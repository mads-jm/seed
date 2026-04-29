---
kanban-plugin: board
tags:
  - index
  - kanban
  - backlog
date created: Wednesday, April 29th 2026, 7:14:00 am
date modified: Wednesday, April 29th 2026, 7:53:29 am
---

# Inbox

# Scoping

- [ ] __TASK-014__ · Close the loop without the TUI [D, L]
  Decide between Option A (`seed log <verb>` CLI subcommand) or Option B (rescope README to "open seed to log completions"). See [[v0-1-0-punch-list#TASK-014 · Close the loop without the TUI [D, L]]].

# Ready

- [ ] __TASK-015__ · Reconcile docs with parser + flags [F]
  README/help drift; missing `?<skill>`, `/random`, `/all <n>`, snooze; `--dev` gate undocumented. See [[v0-1-0-punch-list#TASK-015 · Reconcile docs with parser + flags [F]]].
- [ ] __TASK-018__ · Restore `seed-core` purity [H]
  Move file I/O out of `seed_core::config` so core stays pure. Unblocks TASK-016. See [[v0-1-0-punch-list#TASK-018 · Restore `seed-core` purity [H]]].
- [ ] __TASK-016__ · CONFIG tab — read-only v0.1.0 cut [F, H]
  Add `SideTab::Config` rendering the resolved Config; full editable surface deferred to v0.2.0. See [[v0-1-0-punch-list#TASK-016 · CONFIG tab — read-only v0.1.0 cut [F, H]]].
- [ ] __TASK-020__ · Single source of truth for the wire protocol [H]
  Have `seed-tui` depend on `seed-daemon`'s lib and use `seed_daemon::wire::*` instead of duplicating the protocol. See [[v0-1-0-punch-list#TASK-020 · Single source of truth for the wire protocol [H]]].
- [ ] __TASK-021__ · Fix forward-compat asymmetry [H, L]
  Replace TUI's hand-rolled deserialize with `seed_core::events::from_envelope`. Stops the TUI silently dropping unknown future events. Prereq for TASK-024. See [[v0-1-0-punch-list#TASK-021 · Fix forward-compat asymmetry [H, L]]].
- [ ] __TASK-022__ · Single source of truth for event kinds [H]
  Define event kinds once; derive `is_known_kind` and `event_kind` from that single source. Pairs with TASK-021. See [[v0-1-0-punch-list#TASK-022 · Single source of truth for event kinds [H]]].
- [ ] __TASK-023__ · Schema doc reconciliation [H]
  Doc-side fixes shipped (10 tiers, `LevelUp.old_level`, `seed.reminder.interval_changed`, prestige status flips). Open: code-side enforcement test that fails when schema and `Tier`/`Event` enums drift. Pairs with TASK-022. See [[v0-1-0-punch-list#TASK-023 · Schema doc reconciliation [H]]].
- [ ] __TASK-025__ · Emit `TierChanged` + tier-up toast [L]
  Daemon must emit `TierChanged` on level-up boundaries; TUI shows distinctive toast. Currently `Event::TierChanged` exists but has zero producers. See [[v0-1-0-punch-list#TASK-025 · Emit `TierChanged` + tier-up toast [L]]].
- [ ] __TASK-024__ · Prestige minimum-viable surface [L]
  Token counter on LEVELS tab + "INTEGRATE READY" hint at lvl 99 — or explicit defer. Currently tokens accumulate silently. Depends on TASK-021. See [[v0-1-0-punch-list#TASK-024 · Prestige minimum-viable surface [L]]].
- [ ] __TASK-027__ · `EventLog` durability fixes [H]
  `count_lines` UTF-8 drift, non-atomic Windows rename in `snapshot_write`, dead cfg in `append`. See [[v0-1-0-punch-list#TASK-027 · `EventLog` durability fixes [H]]].
- [ ] __TASK-019__ · Stop TUI from mutating shared `State` [H]
  Introduce `App.client_log` separate from `State.log`; LOG view renders both. See [[v0-1-0-punch-list#TASK-019 · Stop TUI from mutating shared `State` [H]]].
- [ ] __TASK-026__ · Richer overdue indicator [D, L]
  Status bar shows `OVERDUE: N · SNOOZED: M · COMPLETED: K`. Coordinate with TASK-017. See [[v0-1-0-punch-list#TASK-026 · Richer overdue indicator [D, L]]].
- [ ] __Minor / nit batch__
  Bundled cleanup PR after the larger items land: q-key affordance, eprintln in apply_event, double-pinned crossterm, dead pubs, etc. See [[v0-1-0-punch-list#Minor / nit cleanup batch]].
- [ ] __TASK-028__ · Carve `app.rs` and `glyph.rs` [H]
  Modularise `app.rs` (1089 lines) and `glyph.rs::render_glyph` (800 lines). Last, to avoid merge churn. See [[v0-1-0-punch-list#TASK-028 · Carve `app.rs` and `glyph.rs` [H]]].

# In Progress

- [ ] __TASK-013__ · Snooze reachable from TUI [F, D]
  Daemon side exists; needs TUI dispatch (`s` keybinding or `snooze <word>` verb), orbit card visual indicator, parse test. See [[v0-1-0-punch-list#TASK-013 · Snooze reachable from TUI [F, D]]].
- [ ] __TASK-017__ · Coordinate overdue → reset before tag [D, L]
  Data-model + scheduler shipped (`was_snoozed` flag, `traits_skipped` aggregation). Pending: confirm LEVELS `▾N` indicator + skill-detail "Skipped" line render in TUI. See [[v0-1-0-punch-list#TASK-017 · Coordinate overdue → reset before tag [D, L]]].

# In Review

# Done

%% kanban:settings

```
{"kanban-plugin":"board","list-collapse":[]}
```

%%
