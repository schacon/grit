# t3201-branch-contains

## Goal

Make `tests/t3201-branch-contains.sh` pass entirely (24/24).

## Changes (grit `branch`)

- **Filter + modification conflict:** When listing filters are active implicitly (`--contains`, `--no-contains`, `--merged`, `--no-merged`) and the user also passes `-d`/`-m`/`-c`/`-C`, exit 129 with Git-style fatal message (before `-m foo` could succeed).
- **Commit-only revisions:** `--contains` / `--no-contains` / `--merged` / `--no-merged` arguments must peel to commits; tree/blob emit the two-line `error:` messages and exit 129.
- **Multiple flags:** `--merged` and `--contains` repeat with **OR** (union). `--no-merged` repeats with **AND** (intersection). Clap `Append` for `--merged` / `--no-merged`.
- **Upstream on create:** `--track topic main` writes `remote = .` + `merge = refs/heads/main`. Plain `branch zzz topic` does **not** set tracking (matches Git).
- **Verbose listing:** `-v` shows `[ahead 1]` for local upstream when only ahead; `-vv` shows `[main: ahead 1]`. Remote tracking uses `[remote/branch: …]` forms matching Git. `gone` / behind-only cases aligned with observed Git output.

## Verification

- `./scripts/run-tests.sh t3201-branch-contains.sh` → 24/24
- `cargo fmt`, `cargo check -p grit-rs`, `cargo clippy -p grit-rs --fix --allow-dirty`, `cargo test -p grit-lib --lib`
