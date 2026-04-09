# t4041-diff-submodule-option

## Changes

- `diff-index`: `--submodule=log` and `--submodule=diff` now enable submodule handling; `--submodule=short` keeps plain `Subproject commit` hunks only (`SubmodulePatchFormat`).
- After printing `Submodule path … (new submodule)` / `(submodule deleted)` / `(commits not present)`, stop emitting inner tree patches (matches Git).
- Log mode: `Submodule …` header plus `rev_list`-based `  >` / `  <` subject lines for forward, rewind, and divergent ranges.
- Diff mode: unchanged recursive unified diffs inside the submodule.
- Typechange blob↔gitlink: submodule summary and blob patch order aligned with Git for `--submodule=log`.
- Porcelain `git diff`: all gitlink entries with `fmt == "log"` go through `write_patch_entry` with `Log` (not only modified gitlink pairs).
- `submodule_commit_subject_line` moved to `grit-lib::diff` for shared use.

## Validation

Terminal capture was unavailable in this agent session. Run locally:

```bash
cargo fmt && cargo clippy --fix --allow-dirty -p grit-rs
cargo test -p grit-lib --lib
cargo build --release -p grit-rs
./scripts/run-tests.sh t4041-diff-submodule-option.sh
```

## Git

Commit and push on branch `cursor/t4041-diff-submodule-option-3766` if `git status` shows unstaged changes.
