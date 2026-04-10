## Task

Status performance/parity follow-up focused on `t7063-status-untracked-cache.sh` sparse-checkout setup path.

## Context

During Phase 2 continuation, `t7063` sparse setup started failing at:

- `set up sparse checkout` (test 30)
- downstream sparse checks (31+)

The failure message included:

- `warning: unrecognized pattern: 'done/[a-z]*'`
- `warning: disabling cone pattern matching`

and then sparse checkout did not include `done/`, so `done/.gitignore` and `done/two` paths were missing.

## Root cause

Non-cone sparse pattern matching path in `grit-lib/src/sparse_checkout.rs` used a custom
`sparse_glob_match_star_crosses_slash()` helper that only understood `*`/`?` and did not
properly implement bracket-class semantics (e.g. `[a-z]`), which are used by this test.

That caused pattern `done/[a-z]*` to fail matching as intended.

## Change made

Updated `sparse_glob_match_star_crosses_slash()`:

- when pattern contains bracket classes (`[`) or escapes (`\\`), delegate to existing
  `wildmatch(..., flags=0)` implementation instead of the simplified matcher.
- keep existing fast path for simple wildcard-only patterns.

## Validation

Commands run:

- `cargo fmt`
- `cargo check -p grit-rs`
- `cargo build --release -p grit-rs`
- `cp target/release/grit tests/grit && chmod +x tests/grit`
- `bash tests/t7063-status-untracked-cache.sh --run=1-40 -v`
- `bash tests/t7519-status-fsmonitor.sh --run=1-5 -v`

Observed:

- `t7063` sparse setup no longer fails at test 30/31 in properly sequenced runs.
- `t7519` smoke slice (1-5) remains passing.
- Remaining `t7063` failures are still in untracked-cache trace/dump parity after test 20
  (existing Phase 2 parity work), not from sparse non-cone pattern parsing anymore.
