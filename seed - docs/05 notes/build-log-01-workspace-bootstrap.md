---
date created: Tuesday, April 28th 2026, 12:00:00 pm
date modified: Wednesday, April 29th 2026, 7:53:27 am
cssclasses: []
tags:
  - note
  - build-log
  - wave-1
status: archived
---

# Build Log 01 — Workspace Bootstrap

__Tasks__: TASK-001 · __Wave__: 1

## Scope

Bootstrapped the Cargo workspace: three-crate layout (`seed-core` lib, `seed-daemon` → `seedd` bin, `seed-tui` → `seed` bin), workspace-level dependency pinning, stable toolchain, rustfmt defaults.

## What Shipped

Workspace root: `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `.gitignore`. Per-crate: `Cargo.toml` + stub `lib.rs` / `main.rs` for all three crates.

## Technical Decisions

- __Workspace-level dependency pinning__ — all shared crate versions declared in `[workspace.dependencies]`; member crates use `crate.workspace = true`. Stops version drift across crates.
- __`interprocess = "2"`__ — semver-compatible range; resolver locks `2.4.x`. The `tokio` feature exists on 2.x.
- __`rustfmt.toml` is stable-only__ — initial draft had `imports_granularity` and `group_imports` (nightly-only). Removed to keep `cargo fmt --check` warning-free.
- __Pre-declared deps consumed later__ — `notify-rust`, `dirs`, `toml`, `toml_edit`, `unicode-width`, `tracing*`, `serde*`, `chrono` are declared at the workspace root but not pulled into any crate yet. Resolver validates them at lock time; downstream waves consume them.

## See also

- [[build-log-02a-seed-core-foundation]] — Wave 2A built on this scaffold.

