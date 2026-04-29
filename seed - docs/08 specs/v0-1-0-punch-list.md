---
date created: Tuesday, April 28th 2026, 12:00:00 pm
date modified: Wednesday, April 29th 2026, 7:53:24 am
cssclasses: []
tags:
  - spec
  - backlog
  - v0-1-0
  - punch-list
status: active
---

# V0.1.0 Punch List

Source-of-truth prose for the active follow-up work between v0 MVP and the v0.1.0 tag. Tracked on the [[BACKLOG.kanban|backlog board]] as one card per task; details, AC, and reality notes live here.

Flags per task: __D__ = daemon usefulness, __L__ = dopamine loop, __F__ = friction, __H__ = engineering hygiene.

A separate workstream owns __overdue → reset__ (see [[overdue-rollover]]); tasks below avoid overlap and mark coordination points.

## P0 — Release Blockers

### TASK-013 · Snooze Reachable from TUI [F, D]

- __Deps__: TASK-009 (TUI layout shipped)
- __Reality__: `Action::Snooze { reminder_id, minutes }` exists in `seed-daemon/src/wire.rs` + `daemon.rs` handler (~line 293) and is re-declared in `seed-tui/src/client.rs:66`. Zero TUI dispatch — grep finds no `Snooze` in `app.rs`, `command.rs`, `input.rs`, `view/orbit.rs`, `view/skill_detail.rs`.
- __AC__:
  - Keybinding (suggest `s` while a LIST or skill-detail row is selected, or verb form `snooze <word> [minutes]`) sends `IpcAction::Snooze` with default duration from `Config::snooze_min` (already loaded in `app.rs:94`).
  - Orbit pane card visually reflects `snoozed_until_ms > now_ms` (dim + remaining minutes or "ZZZ").
  - One new test in `seed-tui/tests/client.rs` covers the parse / key-map path.

### TASK-014 · Close the Loop without the TUI [D, L]

- __Deps__: TASK-006
- __Reality__: `Event::LevelUp` is only emitted from `daemon.rs::build_complete_events` on user-initiated `Action::Complete`. `schedule.rs` emits `ReminderNotified` and drain-side `TraitXpChanged` only. Background-only users (TUI closed) get notifications but no XP, no level-ups — the loop is gated on the TUI.
- __AC__ (pick one before tagging):
  - __Option A (preferred)__: `seed log <verb>` CLI subcommand on the `seed` binary connects to the daemon, sends `Action::Complete`, prints the resulting XP/level diff, exits. No TUI required.
  - __Option B (scope cut)__: README + OS notification body explicitly say "open `seed` to log completions"; remove "level up" framing from background-use sales copy.
- Whichever lands, README's *Why* + *What you get* must match.

### TASK-015 · Reconcile Docs with Parser + Flags [F]

- __Deps__: none (text + tiny CLI parse)
- __Reality__:
  - README `Commands` lists 9 of 20 reminder verbs; `--help` lists all 20 but mis-orders them. `?<skill>`, `/random`, `/all <n>` are implemented and undocumented in README and [[cli-flags]].
  - README + TASK-009 promise `seed --dev` / `SEED_DEV=1` gate; grep in `crates/seed-tui` returns zero matches for either. `Ctrl+T` tweaks panel is always live, including destructive RESET and unrealistic XP multipliers — exposed to all users.
- __AC__:
  - README "Commands" enumerates all 20 verbs (grouped by category) plus `?<skill>`, `/random`, `/all <n>`.
  - A test parameterised over `REMINDERS.iter().map(|r| r.word)` asserts every word appears in `--help` output (catches drift).
  - Either gate the tweaks panel behind `SEED_DEV=1` / `--dev` (preferred — destructive surface) __or__ remove dev-mode framing from README + TASK-009 wording. No middle ground.

### TASK-016 · CONFIG Tab — Read-only V0.1.0 Cut [F, H]

- __Deps__: TASK-009; TASK-018 (config-purity move) enables this naturally
- __Scope cut__: The full editable + `toml_edit` round-trip + `ConfigChanged` reload set is __deferred to v0.2.0__. v0.1.0 ships a read-only surface that turns hidden knowledge into discoverable knowledge.
- __AC__:
  - Add `SideTab::Config` variant + render fn. Cycle order becomes LIST → LEVELS → LOG → CONFIG → LIST.
  - Renders the resolved `Config`: `active_hours`, `snooze_min`, `palette`, `notif_style`, `glyph_seed`, per-reminder `interval_min` / `enabled` (scrollable).
  - Footer hint: "edit `~/.seed/config.toml` and restart `seedd` to apply changes."
  - No edit affordance, no `Ctrl+S`, no `ConfigChanged` emission. Those land with the full editable spec in v0.2.0.

### TASK-017 · Coordinate Overdue → Reset before Tag [D, L]

- __Deps__: external workstream — see [[overdue-rollover]]
- __Status__: data-model + scheduler shipped 2026-04-29: `Event::ReminderSkipped` carries `was_snoozed: bool`, scheduler emits skip with snooze leniency, `state.traits_skipped` aggregates per-trait stats. LEVELS-tab `▾N` indicator and skill-detail "Skipped: …" line are described in [[overdue-rollover]] and remain the open follow-up — verify they render before tagging.
- __AC__:
  - Confirm the LEVELS / skill-detail render paths from [[overdue-rollover#Per-trait skipped surface]] are wired in `seed-tui/src/view/`.
  - The pre-existing `overdue_reminder_drains_xp` test was split per the spec; check the new tests landed in `crates/seed-daemon/src/schedule.rs`.
  - Coordinate the status-bar surface with TASK-026.

## P0 — Hygiene Blockers (from inspector)

### TASK-018 · Restore `seed-core` Purity [H]

- __Deps__: none; unblocks TASK-016 cleanup
- __Reality__: `seed-core/src/config.rs` violates the [[pure-core]] invariant — `load()` does `fs::read_to_string`, `scaffold_default()` does `create_dir_all` + `write`, `seed_home_path()` reads env. Replay/golden tests depend on core purity.
- __AC__:
  - `seed_core::config::parse(toml: &str) -> Config` (or similar) does only TOML→struct.
  - File reads move to the daemon and to `seed_tui::init` (init scaffold).
  - Drop `seed_core::config::seed_home_path` (duplicates `paths::seed_home`, has zero callers).
  - Drop `dev-dependencies.tempfile` from `seed-core/Cargo.toml` once the I/O tests move out.

### TASK-019 · Stop TUI from Mutating Shared `State` [H]

- __Deps__: none
- __Reality__: `seed-tui/src/app.rs::push_log` (~line 1003) writes to `self.state.log` directly. Eleven call sites across `dispatch_tweak_action` and `submit_command`. Violates the [[event-sourcing]] invariant — TUI is a reader; only the daemon's `commit()` writes through `apply_event`. These client-only log lines vanish on TUI restart.
- __AC__:
  - Introduce `App.client_log: VecDeque<LogEntry>` separate from `State.log`.
  - LOG view renders both (client + daemon) with a visual distinction.
  - `state` field on `App` becomes effectively read-only post-snapshot; flag any remaining direct mutation in review.

### TASK-020 · Single Source of Truth for the Wire Protocol [H]

- __Deps__: none
- __Reality__: `seed-tui/src/client.rs:1-138` reimplements `Message`, `Action`, `ResponseResult`, `read_frame`, `write_frame`, `MAX_FRAME`, `socket_name`. The "daemon is binary-only" justification comment is __false__ — `seed-daemon` already exposes a lib (`Cargo.toml:8-10`) and `tests/smoke.rs` imports from it. Three copies of `socket_name` exist.
- __AC__:
  - `seed-tui` depends on `seed-daemon = { path = "…" }` and `use seed_daemon::wire::*`.
  - Delete the duplicates in `client.rs`. Adjust the comment.

### TASK-021 · Fix Forward-compat Asymmetry [H, L]

- __Deps__: TASK-020
- __Reality__: Daemon persistence routes unknown event kinds to `Event::Unknown` (preserving payload — the [[wire-versioning]] guarantee). TUI's `app.rs::apply_envelope` (~line 948) hand-rolls `serde_json::from_value::<CoreEvent>` and silently drops on `Err`. A TUI talking to a newer daemon desyncs without warning. Compounds with the missing prestige UI in TASK-024.
- __AC__:
  - Replace the hand-rolled deserialize with `seed_core::events::from_envelope(env)`.
  - `apply_event` already handles `Event::Unknown` as a no-op — the reader path becomes correct by construction.
  - Add a test that feeds the TUI a synthetic future-kind envelope and asserts the local mirror still applies the rest of a batch.

## P1 — Should-fix before Tag

### TASK-022 · Single Source of Truth for Event Kinds [H]

- __Deps__: none, but pairs with TASK-021
- __Reality__: Each event kind string is hard-coded in three places — `#[serde(rename = …)]` per variant in `seed-core/src/events.rs:31-135`, `is_known_kind` (`:198-218`), `event_kind` (`:220-245`). Adding a new variant compiles clean but `from_envelope` routes the kind to `Unknown`. `event_kind` for `Event::Unknown` returns the literal `"seed.unknown"` and discards the original kind string — wrong.
- __AC__:
  - Define kinds once (e.g. `const KINDS: &[(EventDiscriminant, &str)]` or a derived accessor).
  - `is_known_kind` and `event_kind` are derived from that single source.
  - Add a doc-test or integration test that asserts every variant in `Event` appears in [[events-schema]].

### TASK-023 · Schema Doc Reconciliation [H]

- __Deps__: TASK-022 ideally
- __Status__: doc-side fixes landed 2026-04-29 in the post-migration pass. Code-side enforcement (the test that fails when schema and `Tier` enum drift) remains.
- __Reality__:
  - ~~[[events-schema]] lists 7 tiers `SEED → … → ZENITH`. `seed-core/src/domain.rs:466-477` defines __10__.~~ Fixed: schema now lists all 10 with thresholds — see [[tier-progression]].
  - ~~`seed.trait.level_up` schema does not mention `old_level: u8`.~~ Fixed: documented with `#[serde(default)]` semantics.
  - ~~`Event::ReminderIntervalChanged` (`seed.reminder.interval_changed`) missing from schema.~~ Fixed: documented.
  - ~~Prestige specs `status: draft` despite data models being implemented.~~ Fixed: both flipped to `partial` with implementation banners.
  - Backlog TASK-007 wording about XP drain being a "separate slower tick" was already corrected during the migration to [[v0-mvp]].
- __AC remaining__:
  - One test asserts every `Tier::name()` appears in [[events-schema]] (or the doc is generated from the enum). Same for every `Event` variant kind string.
  - Pairs naturally with TASK-022 (single source of truth for event kinds): the test that closes both is the same shape — derive doc references from the enum, fail on drift.

### TASK-024 · Prestige Minimum-viable Surface [L]

- __Deps__: TASK-021
- __Reality__: `Event::TraitIntegrated`, `Event::FocusTokenEarned`, `Event::FocusPhaseActivated` exist. State carries `cumulative_levels_gained`, `tokens_spent`, `active_focus`, `trait_integrations`, `trait_enhancements`. Daemon emits `FocusTokenEarned` correctly on level-up boundaries (`daemon.rs:605-615`). Grep `prestige|FocusToken|TraitIntegrated|FocusPhase` in `seed-tui/src` returns __zero__ matches. Tokens accumulate silently; level-99 traits cannot integrate; the long-tail dopamine the specs lean on is dead.
- __Status__: prestige specs ([[prestige-integrate]], [[prestige-focus]]) flipped to `status: partial` on 2026-04-29 with explicit "data model implemented, UI deferred to v0.2.0" banners. The cut is documented; the __minimum-viable hint__ is still open work below.
- __AC__:
  - LEVELS tab footer (or status bar) shows `tokens: N` when `tokens_available(state) > 0`.
  - At least one trait at lvl 99 surfaces an "INTEGRATE READY" hint.
  - These two are the v0.1.0 minimum. Full UI lives in v0.2.0.

### TASK-025 · Emit `TierChanged` + Tier-up Toast [L]

- __Deps__: none
- __Reality__: `Event::TierChanged { from, to, total_level }` exists with snapshot tests but no producer — `apply_event` is a no-op for it (`events.rs:482`), and no daemon path emits it. Grep confirms zero producers in `seed-daemon/src`. Tier crossings are the natural macro celebration moments (~9 across the whole companion lifetime); they're currently invisible.
- __AC__:
  - `daemon.rs::build_complete_events` (or a sibling) computes `tier_for(old_total)` vs `tier_for(new_total)` after a level-up; emits `TierChanged` on transition.
  - TUI shows a distinctive toast for tier changes (separate `ToastKind` from `LevelUp`).
  - Round-trip + emission test.

### TASK-026 · Richer Overdue Indicator [D, L]

- __Deps__: coordinate with TASK-017
- __Reality__: `seed-tui/src/view/status_bar.rs:14` exposes a single `any_overdue: bool`; status bar reads "COMPANION WILTING" with no count, no list, no urgency gradient. Combined with the missing snooze (TASK-013) and overdue→reset reshape, the user has near-zero context.
- __AC__:
  - Status bar shows `OVERDUE: N · SNOOZED: M · COMPLETED: K` (or equivalent).
  - Coordinated with whatever shape TASK-017 lands.

## P1 — Hygiene (from inspector)

### TASK-027 · `EventLog` Durability Fixes [H]

- __Deps__: none
- __Reality__: the [[snapshot-and-replay]] pattern relies on the event log being byte-honest; these issues poke holes in that contract.
  - `event_log.rs::count_lines` (~:143-150) consumes `reader.lines()` regardless of `Ok`/`Err`. Bad-UTF-8 lines increment count by 1 but `load_from` (~:89-110) skips them with `warn!`. Snapshot encodes drifted `event_count`; on restart `load_from(_, skip=N)` skips past valid events. Subtle data loss.
  - `snapshot_write` (~:115-127) uses `fs::rename(tmp, path)` which is __not atomic on Windows__ when the destination exists.
  - `EventLog::append` (~:71-74) has two identical `#[cfg(target_os = "windows")]` / `#[cfg(not(…))]` branches calling the same `sync_data()`. Dead branching.
- __AC__:
  - `count_lines` returns `Result<usize, io::Error>` and counts only `Ok` lines (or is replaced by tracking the count alongside `append`).
  - Snapshot uses `tempfile::persist` (or a wrapper around `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`).
  - Delete the redundant cfg in `append`.
  - Test: corrupt-line-mid-log replay + restart preserves `event_count`.

### TASK-028 · Carve `app.rs` and `glyph.rs` [H]

- __Deps__: ideally lands after TASK-019/020/021 to avoid merge churn
- __Reality__:
  - `seed-tui/src/app.rs` is 1089 lines doing terminal setup + IPC subscribe + render orchestration + 8 sidebar nav handlers + command parser + tweaks dispatch + skill-detail dispatch + level-up toast policy + level computation + request-id alloc + a splitmix64 RNG. `#[allow(dead_code)]` on the struct (line 58) silences the linter wholesale.
  - `seed-core/src/glyph.rs::render_glyph` is 800 lines of inlined per-layer cell math (11 layers + symmetry mirror + frame materialization). A local `fn rgb` is even defined inside the function body — already an admission this should be modularized.
- __AC__:
  - `app.rs` carved into at minimum `app/input.rs`, `app/render.rs`, `app/dispatch.rs`. Move `random_trait_levels` to a debug-only module. Replace the struct-level `#[allow(dead_code)]` with per-field attributes (or delete dead fields).
  - `glyph.rs` introduces a `LayerRenderer` trait or sibling module per layer; `render_glyph` becomes a short composition.
  - No behaviour change; golden snapshot must still byte-match. Run `cargo test -p seed-core --test glyph` as the sign-off.

## Minor / Nit Cleanup Batch

These are small enough to bundle as a single PR after the larger items land:

- Status bar advertises `[ Q ]` quit while `input.rs:77-78` says bare `q` is intentionally not quit in the main view. Fix the affordance text or harmonise the binding.
- Six `#[cfg(debug_assertions)] eprintln!` calls inside `seed_core::events::apply_event` (`:382-468`) — corrupts TUI screen, breaks core purity. Return `Result<()>` from the validation arms and let the daemon log.
- `crossterm` is double-pinned (workspace + `seed-tui/Cargo.toml:26`); `futures` slipped into `seed-tui` only (line 28). Lift to `[workspace.dependencies]`.
- `view/toast.rs:69` width uses `msg.len()` (bytes); use `unicode-width::UnicodeWidthStr::width` (already a workspace dep).
- `IpcClient::send` (`client.rs:201-205`) returns `()`, swallowing backpressure. At minimum return `Result<(), SendError>` so callers can choose to surface "command dropped during reconnect."
- `daemon::Action::Reset` (`daemon.rs:447-501`) hand-rolls everything `commit()` does. Route through `commit()`.
- Dead pubs / dead-code: `seed_core::glyph::weighted_char_pub`, `seed_core::config::seed_home_path` (duplicates `paths::seed_home`), `seed_core::paths::config_path` (TUI's `init.rs:11` reimplements it inline), `view/title_bar.rs:29` `let _ = tier_for(…)`, `view/command_bar.rs:37` `let _ = color`, `schedule.rs:91-92` discarded `level`.
- `crates/seed-core/tests/seed_home_env_override.rs:12-17` SAFETY note is wrong (not "tests are single-threaded by default"); rewrite to "this test binary contains exactly one test."
- `#[ignore]` without note: `crates/seed-core/tests/glyph.rs:339` (`perf_glyph_159x79`), `:368` (`dump_golden`). Add a note (e.g. `#[ignore = "perf, run with --ignored"]`).
- `crates/seed-daemon/src/daemon.rs:71-72` order: `create_dir_all` after `EventLog::open` — if `seed_home` doesn't exist, `snapshot_read` fails first. Move `create_dir_all` above `snapshot_read`.
- Trait-id strings hardcoded twice (`seed-core/src/state.rs:130-167` vs `seed-core/src/glyph.rs:155-163`). Define once in `domain.rs`.
- Pin-mark inconsistency: `view/orbit.rs` uses `"*"`, `view/side_panel.rs` uses `"★"`. Pick one (with the existing braille fallback pattern).
- Triple `KeyEventKind::Press | KeyEventKind::Repeat` filter (`app.rs:373-376, :408-411`, `input.rs:55-58`). One helper.

## Cross-cutting Threads

1. __Event-kind discipline__ — TASK-022 + TASK-023 + TASK-025 are one thread; close them together so the schema, the Rust enum, and the producers stay in lockstep.
2. __Forward-compat asymmetry__ — TASK-021 is a prereq for TASK-024; until the TUI stops dropping unknown events, adding the prestige UI surface paves over the bug.
3. __Config purity ↔ CONFIG tab__ — TASK-018 is the natural moment to do TASK-016, since the TUI becomes the config consumer regardless.

## Suggested Cut order (smallest Credible v0.1.0)

1. TASK-015 (docs / dev-flag) — text + tiny CLI parse, zero risk.
2. TASK-013 (snooze) — small, daemon side already there.
3. TASK-018 → TASK-016 (config purity, then read-only CONFIG tab).
4. TASK-020 → TASK-021 (single-source wire, then `from_envelope` in TUI).
5. TASK-022 + TASK-023 + TASK-025 (event-kind discipline thread).
6. TASK-024 minimum (token counter or explicit defer).
7. TASK-027 (event-log durability).
8. TASK-014 (loop closure decision — CLI or doc reframe).
9. TASK-019 (`push_log` separation).
10. TASK-026 + TASK-017 (coordinate overdue surface with overdue→reset workstream).
11. Minor / nit batch as one PR.
12. TASK-028 (carve modules) — last, to avoid merge churn.

## See also

- [[v0-mvp]] — historical plan for the v0 MVP (TASK-001..012, all shipped).
- [[BACKLOG.kanban]] — active board; one card per task here.
- [[overdue-rollover]] — the overdue → reset spec coordinated by TASK-017.
- [[xp-pacing]], [[prestige-integrate]], [[prestige-focus]] — referenced by TASK-023 / TASK-024.
- [[events-schema]] — the wire-schema doc TASK-022 / TASK-023 reconcile.
- [[event-sourcing]], [[pure-core]], [[wire-versioning]], [[snapshot-and-replay]] — the invariants several tasks here are restoring.
