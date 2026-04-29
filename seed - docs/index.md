---
tags:
  - index
date created: Wednesday, April 29th 2026, 7:01:57 am
date modified: Wednesday, April 29th 2026, 7:53:22 am
---

# Seed Documentation

> __For LLMs__: Start here. Navigate to a directory index for scoped exploration, or jump directly to a core document below. [[CLAUDE]]

> __For Humans:__ See above… but use the graph to guide your exploration!

---

## Vault Structure

| Directory | Index | Contents |
|-----------|-------|----------|
| `00 index/` | `this note` | Root navigation hub for the vault |
| `01 concepts/` | [[CONCEPTS]] | Atomic concept notes and durable project knowledge |
| `02 references/` | [[REFERENCES]] | Library API references and external docs |
| `03 guides/` | [[GUIDES]] | Developer workflow and implementation guides |
| `04 architecture/` | [[ARCHITECTURE]] | Design spec, ADRs |
| `05 notes/` | [[NOTES]] | Fleeting notes and working notes |
| `06 reports/` | - | Sprint reports and progress snapshots |
| `07 stories/` | [[STORIES]] | Vision and manifesto |
| `08 specs/` | [[SPECS]] | Feature and component specifications |
| `09 milestones/` | [[MILESTONES]] | Release and milestone summaries |
| `10 PRs/` | [[PRS]] | PR history |
| `99 meta/` | - | Templates and vault maintenance material |

`.obsidian/` is vault configuration and snippet state, not part of the documentation corpus. `docs/` at the project root is the GitHub Pages export target — it is build output, not source.

---

## Core Documents

- [[README]]
- [[v0-mvp]] — historical record of the v0 MVP build (locked decisions, TASK-001..012, prestige pre-wiring).
- [[v0-1-0-punch-list]] — full prose for the active backlog (TASK-013..028, cross-cutting threads, suggested cut order).
- [[BACKLOG.kanban]] — active board, one card per active task.
- [[events-schema]] — wire-protocol contract.
- [[cargo-cheatsheet]] — cargo workflow reference.
- [[ARCHITECTURE]] — workspace layout, crate boundaries, event/IPC design.
- [[ADR]] — architectural decision records.
- [[SPECS]] — feature and component specifications index.
- [[CONCEPTS]] — durable patterns referenced across the vault. The eight planned notes are listed there as unresolved wikilinks until written.

---

## Quick Reference

### Common Commands

```bash
cargo build                 # build the workspace
cargo test                  # run all tests
cargo run -p seed-tui       # launch the TUI (auto-spawns seedd if absent)
cargo run -p seed-daemon -- --foreground   # run the daemon in foreground
SEED_LOG=debug cargo run -p seed-tui       # verbose logging
```

### Key File Locations

| Area | File |
|------|------|
| Workspace manifest | `Cargo.toml` |
| Core domain + pure logic | `crates/seed-core/src/` |
| Daemon (IPC + scheduler + event log) | `crates/seed-daemon/src/main.rs` |
| TUI (ratatui client) | `crates/seed-tui/src/main.rs` |
| State directory | `~/.seed/` (override via `SEED_HOME`) |
| Event log | `~/.seed/events.jsonl` |
| IPC socket (Unix) | `~/.seed/seedd.sock` |
| IPC pipe (Windows) | `\\.\pipe\seedd-<user>` |
| User config | `~/.seed/config.toml` |

---

__Last Updated__: 2026-04-29
__Documentation Version__: v0.1.0





