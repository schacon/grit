# AGENTS.md — Working on Grit

Grit is a from-scratch reimplementation of Git in idiomatic, library-oriented Rust.
The goal: pass the entire upstream Git test suite.

## Quick Start

```bash
# Build
cargo build --release

# Run a single test file
./scripts/run-tests.sh t3200-branch.sh

# Run a category
./scripts/run-tests.sh t1

# See what's failing
./scripts/run-tests.sh --failing
```

## The One Rule

**Fix grit Rust code to make upstream tests pass. Do not modify tests.**

The only exception: flipping `test_expect_failure` → `test_expect_success` when
you've fixed the underlying bug.

## How to Work

Read **TESTING.md** for the full strategy. The short version:

1. Pick **one test file** that isn't fully passing
2. Run it, study the failures
3. Fix the Rust code (`grit/src/` or `grit-lib/src/`)
4. Rebuild (`cargo build --release`)
5. Re-run until fully passing
6. Update results: `./scripts/run-tests.sh <file>`
7. Commit with a message like `fix: make t1234-foo fully pass`

### Priority Order

Plumbing first (t0-t1), then core commands (t2-t3), diff (t4), transport (t5),
rev machinery (t6), porcelain (t7), external helpers (t9) last.

Within each category: files closest to fully passing first (quick wins).

### Before Committing Rust Code

```bash
cargo check -p grit-rs 2>&1 | grep warning    # fix ALL warnings
cargo test -p grit-rs --lib                     # unit tests must pass
```

## Project Structure

```
grit/
├── grit/src/commands/     # Git command implementations
├── grit-lib/src/          # Core library (repo, index, diff, merge, etc.)
├── tests/                 # Ported upstream test files + test-lib.sh
├── git/t/                 # Upstream Git test suite (reference only)
├── data/                  # Test results TSVs (updated by run-tests.sh)
├── docs/                  # Dashboard HTML files
├── scripts/               # Test runner and dashboard generators
└── TESTING.md             # Full testing strategy
```

## Data Flow

```
run-tests.sh → data/file-results.tsv (source of truth)
                    ↓
         extract-and-test.py → test-results.tsv + command-status.tsv
                    ↓
         generate-progress-html.py → docs/index.html
         generate-testfiles-html.py → docs/testfiles.html
```

## Do Not

- Modify `tests/test-lib.sh` (causes regressions)
- Create stub/partial test files (use full upstream tests)
- Skip tests by adding `SKIP` prereqs (fix the code instead)
- Run `cargo build` in worktrees (build in main repo, copy binary)

## Cursor Cloud specific instructions

- **Rust toolchain**: The pre-installed Rust may be outdated. The update script runs `rustup update stable && rustup default stable` to ensure the latest stable toolchain is available, since newer workspace dependencies (e.g. `time-core`) require edition 2024 support (Rust ≥ 1.85).
- **No external services**: Grit is a pure CLI tool with no databases, containers, or network services. Build and test entirely via Cargo and the Bash test runner.
- **Unit tests**: Run `cargo test -p grit-lib --lib` (95 tests). The `grit-rs` crate has no lib target; use `cargo test --workspace` to run everything.
- **Integration tests**: Use `./scripts/run-tests.sh <test-file>` (see TESTING.md). Many tests are expected to fail — Grit is a work-in-progress.
- **Lint**: `cargo check -p grit-rs 2>&1 | grep warning` — there are 2 pre-existing unused-variable warnings in `grit/src/commands/add.rs`.
- **Binary location**: After `cargo build --release`, the binary is at `target/release/grit`. The test harness expects this path.
