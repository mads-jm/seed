---
tags:
  - index
---

# seed Documentation

> __For LLMs__: Start here. Navigate to a directory index for scoped exploration, or jump directly to a core document below.

> __For Humans:__ See above... but use the graph to guide your exploration!

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
| `09 milestones/` | - | Release and milestone summaries |
| `10 PRs/` | [[PRS]] | PR history |
| `99 meta/` | - | Templates and vault maintenance material |

`.obsidian/` is vault configuration and snippet state, not part of the documentation corpus.

---

## Core Documents

- __`docs/backlog.md`__ (project root) — v0 MVP backlog with locked decisions, tasks, and execution waves
- __`docs/build-log/`__ (project root) — per-milestone execution records
- [[ARCHITECTURE]] — workspace layout, crate boundaries, event/IPC design
- [[ADR]] — architectural decision records
- [[SPECS]] — feature and component specifications

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

__Last Updated__: 2026-04-22
__Documentation Version__: v0.1.0
