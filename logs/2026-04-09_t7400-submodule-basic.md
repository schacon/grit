# t7400-submodule-basic

## Goal

Make `tests/t7400-submodule-basic.sh` pass entirely under the grit harness (124 tests).

## Changes

- **`grit/src/main.rs`**: `git submodule -h` exits **0** (not 129) like Git; clap help paths for submodule also exit 0. Dispatch uses `submodule::run_from_argv`.
- **`grit/src/commands/submodule.rs`**: Pre-parse leading `--quiet`/`-q` and `--cached` before clap; reject other leading flags with upstream synopsis on **stderr** and exit **1**; handle bare `--` / `--end-of-options`; add `--cached` status (index vs `HEAD` tree); add `-q`/`--quiet` on subcommand structs and suppress informational output; `run_from_argv` entry.
- **`grit/src/commands/rm.rs`**: Stop removing `[submodule "..."]` from `.git/config` when removing a gitlink — matches Git so `git config remove-section submodule.<name>` in test cleanups succeeds.

## Verification

```bash
export PATH="/usr/local/cargo/bin:/usr/local/rustup/toolchains/1.83.0-x86_64-unknown-linux-gnu/bin:$PATH"
cargo build --release -p grit-rs
cargo test -p grit-lib --lib
./scripts/run-tests.sh t7400-submodule-basic.sh
```

Full file run: 124/124 ok.
