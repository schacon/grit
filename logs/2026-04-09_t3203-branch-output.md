# t3203-branch-output

## Summary

Made `grit branch` match upstream listing/format/sort/color behavior for `tests/t3203-branch-output.sh` (41 pass, 1 skip TTY).

## Key changes

- **`grit-lib` `state.rs`**: `dwim_detach_label` no longer substitutes a tag name when the reflog target is an abbreviated OID (fixes detached-at vs tag label ordering).
- **`checkout`**: `detach_head` takes optional reflog `to` label; tag/remote checkouts record tag or `remote/branch` in `logs/HEAD`; resolve tag by name when branch missing.
- **`branch`**: symref-aware listing (`->`), `--points-at` excluding symrefs whose target matches, multi `--sort`, `ahead-behind`/`objectsize`/`type`/`version:refname`, `-i`, `--omit-empty`, hidden `--no-remotes`/`--no-all`, glob + `-v` fatal, format subset in `branch_ref_format.rs` (`%(if)`, `%(ahead-behind:HEAD)`, `%(color:…)`, `%(rest)` fatal), color/format reset, worktree `+` and path in `-vv`, verbose column width includes detached description, `core.abbrev` for `-v` OIDs.
- **`main.rs`**: `mod branch_ref_format`.

## Validation

- `./scripts/run-tests.sh t3203-branch-output.sh` — 41/41 pass (1 skip).
- `cargo fmt`, `cargo clippy`, `cargo test -p grit-lib --lib`.
