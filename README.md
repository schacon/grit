# Grit — Git in Rust

Grit is a **from-scratch reimplementation of Git** in idiomatic Rust. The goal is to match Git's behavior closely enough that the upstream test suite (under `git/t/`) can be ported and run against this tool.

This implementation is being written entirely by AI coding agents. The AGENT.md instructions and a snapshot of the Git source code were provided, and autonomous agents (first Cursor, then OpenClaw orchestrating Claude Code) implement commands, port tests, and validate against the upstream Git test suite.

## Crates

| Crate | Description |
|-------|-------------|
| [`grit-rs`](https://crates.io/crates/grit-rs) | The `grit` binary — a drop-in CLI reimplementation of `git` with 140+ commands |
| [`grit-lib`](https://crates.io/crates/grit-lib) | Core library: object model, diff engine, index, refs, revision walking, merge, config, and more |

## Progress

See the **[project dashboard](https://schacon.github.io/grit)** (generated from `data/test-files.csv` via `scripts/run-tests.sh`).


