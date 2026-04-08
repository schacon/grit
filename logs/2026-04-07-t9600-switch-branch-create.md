# t9600-switch-branch-create

## Failures (before)

1. `switch --discard-changes <branch>` — `--discard-changes` was treated as a revision; needed `git switch` flag + checkout `-f` before branch name.
2. `switch -c "bad..name"` — invalid ref accepted; needed same validation as upstream (`fatal: '…' is not a valid branch name`, exit 128).

## Changes

- `grit switch`: `--discard-changes` forwards `-f` before positional args.
- `grit checkout`: strip `-f`/`--force` from delegated `rest` into `args.force`.
- `create_and_switch_branch`, `force_create_and_switch_branch`, `create_orphan_branch`: validate `refs/heads/<name>` via `check_refname_format` with `allow_onelevel`.

## Verify locally

```bash
cargo fmt && cargo clippy -p grit-rs -- -D warnings && cargo test -p grit-lib --lib
cargo build --release -p grit-rs
./scripts/run-tests.sh t9600-switch-branch-create.sh
```
