## Task
- Target: `t4107-apply-ignore-whitespace.sh`
- Status: completed (`11/11`)

## Baseline
- `./scripts/run-tests.sh t4107-apply-ignore-whitespace.sh` reported `7/11`.
- Local direct run failures:
  - unsupported CLI options: `--ignore-whitespace`, `--ignore-space-change`, `--no-ignore-whitespace`
  - missing config behavior: `apply.ignorewhitespace=change`
  - final `--ignore-space-change --inaccurate-eof` case left an unexpected trailing newline

## Root causes
1. `grit apply` parsed only `--whitespace=<action>` and did not expose compatibility flags expected by the upstream test.
2. Hunk matching logic only supported exact or `--whitespace=fix` trailing-whitespace normalization; it lacked “ignore whitespace amount” matching mode.
3. `--inaccurate-eof` flag was parsed but never wired into hunk output reconstruction, so newline behavior at EOF could not change.

## Implementation summary
- Added `grit apply` CLI flags:
  - `--ignore-whitespace`
  - `--ignore-space-change`
  - `--no-ignore-whitespace`
- Added apply-time whitespace mode resolution:
  - command-line overrides (`--no-ignore-whitespace` disables ignoring)
  - config fallback from `apply.ignorewhitespace` (`change`/truthy values)
  - existing `--whitespace=fix` behavior remains supported
- Extended context/remove line matching:
  - added whitespace-collapsing normalization for `ignore-space-change`
  - retained existing trailing-whitespace normalization for `--whitespace=fix`
- Ensured context lines copied from matched source content when not in `--whitespace=fix` mode to preserve file formatting.
- Wired `--inaccurate-eof` through hunk application functions so EOF reconstruction can suppress forced trailing newline in that mode.
- Propagated resolved whitespace/EOF mode into worktree apply, index apply, and `--check` paths.

## Validation
- `cargo build --release` ✅
- `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash tests/t4107-apply-ignore-whitespace.sh` ✅ `11/11`
- `./scripts/run-tests.sh t4107-apply-ignore-whitespace.sh` ✅ `11/11`
- `bash scripts/run-upstream-tests.sh t4107-apply-ignore-whitespace` ✅ `11/11`
- `cargo fmt` ✅
- `cargo clippy --fix --allow-dirty` ✅ (unrelated autofixes reverted)
- `cargo test -p grit-lib --lib` ✅
