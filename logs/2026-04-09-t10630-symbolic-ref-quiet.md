# t10630-symbolic-ref-chain-extra

## Issue

Test `symbolic-ref -q on non-symbolic ref fails quietly` failed: `grit symbolic-ref -q refs/heads/master` printed `fatal: No such ref: refs/heads/master` to stderr. Git exits 1 with no stderr in quiet mode.

## Fix

`read_symbolic_ref_target` treated `None` from `read_symbolic_ref_target_maybe_missing` as "need fallback" and always bailed with "No such ref" when the ref file was not `Ref::Symbolic`. For a **direct** ref (OID), `None` means "not a symbolic ref", so the command should return `Ok(None)` and let the caller exit quietly with `-q`.

## Validation

- `sh t10630-symbolic-ref-chain-extra.sh` from `tests/` with `GUST_BIN` → 35/35
- `./scripts/run-tests.sh t10630-symbolic-ref-chain-extra.sh`
- `cargo clippy -p grit-rs`, `cargo test -p grit-lib --lib`
