# Gust

Gust is a **from-scratch reimplementation of Git** in idiomatic Rust. The goal is to match Git’s behavior closely enough that the upstream test suite (under `git/t/`) can be ported and run against this tool.

**v1** focuses on **plumbing** only—commands like `init`, `hash-object`, `cat-file`, the index and tree tools, `commit-tree`, and `update-ref`—not the full porcelain CLI. See `AGENT.md` for the full contract and `plan.md` for the task breakdown.

The reference Git source and tests live in the `git/` directory.
