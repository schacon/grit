# t7500-commit-template-squash-signoff

## Failures (before)

1. `commit -C empty respects --allow-empty-message` — `-C <tag>` was consumed by `strip_subcommand_leading_change_dir` as `git -C <dir>`, causing chdir to missing path.
2. `--fixup=amend: --only ignores staged changes` — `commit` rejected `--only` without pathspec; Git allows this for `fixup=amend:`.

## Fixes

- `grit/src/main.rs`: skip leading `-C` directory stripping for subcommand `commit` (same as switch/checkout).
- `grit/src/commands/commit.rs`: allow `--only` with empty pathspec when `--fixup=amend:` (AmendStyle, not reword).

## Verification

- `./scripts/run-tests.sh t7500-commit-template-squash-signoff.sh` — 57/57 pass.
- `cargo test -p grit-lib --lib` — pass.
