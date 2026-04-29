---
date created: Tuesday, April 28th 2026, 12:00:00 pm
date modified: Wednesday, April 29th 2026, 7:53:29 am
cssclasses: []
tags:
  - guide
  - cargo
  - workflow
status: draft
---

# Cargo Cheatsheet

The workspace ships two binaries and one library:

| Crate         | Kind | Binary name | Path                        |
|---------------|------|-------------|-----------------------------|
| `seed-tui`    | bin  | `seed`      | `crates/seed-tui/src/main.rs` |
| `seed-daemon` | bin  | `seedd`     | `crates/seed-daemon/src/main.rs` |
| `seed-core`   | lib  | —           | `crates/seed-core/src/lib.rs`   |

There is __no `serve` binary__. If you tried `cargo run --bin serve` (or any
shorthand like `cargo run -- serve`), use `seedd` instead.

---

## Build

```bash
cargo build --workspace                # debug build, all crates
cargo build --release --workspace      # release build (use for "real" runs)
cargo build -p seed-tui                # one crate
cargo check --workspace                # typecheck only — fastest feedback loop
```

Output binaries: `target/{debug,release}/seed{,d}{,.exe}`.

## Run

The TUI auto-spawns the daemon if one isn't already running, so you usually
just `cargo run --bin seed`. Run them separately when you want logs.

```bash
cargo run --bin seed                   # launch the TUI
cargo run --bin seed -- init           # scaffold ~/.seed/config.toml
cargo run --bin seed -- --help         # CLI help

cargo run --bin seedd                  # daemon (logs to ~/.seed/seed.log)
cargo run --bin seedd -- --foreground  # daemon, logs to stderr
cargo run --bin seedd -- --version

# Override the state directory (useful for sandboxed dev runs):
cargo run --bin seed -- --seed-home /tmp/seed-dev
SEED_HOME=/tmp/seed-dev cargo run --bin seed
```

Anything after `--` is forwarded to the binary; flags before `--` go to cargo.

### Useful Env Vars

| Var             | Effect                                                      |
|-----------------|-------------------------------------------------------------|
| `SEED_LOG`      | `tracing` filter — `error` / `warn` / `info` / `debug` / `trace` or per-module like `seed_daemon=debug` |
| `SEED_HOME`     | Override `~/.seed/` (config, logs, state, sockets)          |
| `RUST_BACKTRACE`| `1` for short backtrace, `full` for full trace on panic     |
| `RUSTFLAGS`     | e.g. `-C target-cpu=native` for release perf experiments    |

```bash
SEED_LOG=debug cargo run --bin seedd -- --foreground
RUST_BACKTRACE=1 cargo run --bin seed
```

## Test

```bash
cargo test --workspace                 # all unit + integration tests
cargo test -p seed-core                # one crate
cargo test -p seed-tui --tests         # only integration tests in tests/
cargo test -p seed-core --lib          # only inline unit tests
cargo test golden_snapshot             # one test by name (substring match)
cargo test glyph::tests::               # by module path prefix
```

`-- --nocapture` lets `println!` reach stdout (otherwise tests swallow it):

```bash
cargo test -p seed-core glyph -- --nocapture
```

### Ignored Tests

Some tests are gated `#[ignore]` because they spawn real processes or write
golden snapshots. Run them explicitly:

```bash
cargo test --test smoke -- --ignored                            # daemon spawn smoke
cargo test -p seed-core --test glyph dump_golden -- --ignored --nocapture
```

### Refreshing the Glyph Golden Snapshot

When you intentionally change the renderer:

1. `cargo test -p seed-core --test glyph dump_golden -- --ignored --nocapture`
2. Copy the lines between `--- BEGIN SNAPSHOT ---` and `--- END SNAPSHOT ---`
3. Paste them into `crates/seed-core/tests/glyph_golden.txt` (no trailing
   blank line content — only one trailing newline).
4. `cargo test -p seed-core golden_snapshot` must pass.

## Lint, Format, Fix

```bash
cargo fmt --all                        # apply rustfmt
cargo fmt --all -- --check             # check only (CI-friendly)
cargo clippy --workspace -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings   # also lint test code
cargo fix --workspace --allow-dirty    # apply auto-fixes (review the diff)
```

## Inspect

```bash
cargo tree -p seed-tui                 # dep tree for one crate
cargo tree -d                          # show duplicate deps across versions
cargo doc --workspace --no-deps --open # generate + open API docs
cargo expand -p seed-core glyph        # macro-expanded source (needs cargo-expand)
```

## Clean

```bash
cargo clean                            # blow away ./target  (will trigger full rebuild)
cargo clean -p seed-tui                # one crate's artefacts only
```

---

## Common Workflows

### Tight Inner Loop while Editing One Crate

```bash
cargo check -p seed-tui                # ~1-3s typecheck
cargo test  -p seed-tui --tests        # run that crate's tests
```

### Reproducing a Daemon Issue

```bash
# Terminal 1 — verbose daemon in foreground
SEED_LOG=debug cargo run --bin seedd -- --foreground

# Terminal 2 — TUI talks to that daemon
cargo run --bin seed
```

If the TUI keeps spawning a *second* daemon, it means the IPC socket /
named pipe is missing or stale. Stop both, delete `~/.seed/seedd.sock` (or
the equivalent named pipe on Windows) and retry. On Windows the pipe lives
under `\\.\pipe\` and is cleaned up automatically when the daemon exits.

### Sandboxed Run that Won't touch Your Real `~/.seed`

```bash
SEED_HOME=/tmp/seed-sandbox cargo run --bin seed -- init
SEED_HOME=/tmp/seed-sandbox cargo run --bin seed
```

### CI-equivalent Local Check

```bash
cargo fmt --all -- --check && \
cargo clippy --workspace --all-targets -- -D warnings && \
cargo test --workspace
```

## Troubleshooting

- __`error: no bin target named X`__ — only `seed` and `seedd` exist; see the
  table at the top.
- __TUI hangs on launch__ — daemon is unreachable. Run it manually with
  `--foreground` and watch the logs.
- __Logs are empty__ — default log level is `warn`. Set `SEED_LOG=info` (or
  finer) before running.
- __Tests pass locally, fail in CI on golden snapshot__ — golden file has a
  trailing-whitespace or line-ending issue. Regenerate it via the steps above
  and commit only the LF-normalised file.
- __`linker LNK1181` / lld errors on Windows__ — usually a stale `target/`
  directory after a toolchain upgrade. `cargo clean` and rebuild.
