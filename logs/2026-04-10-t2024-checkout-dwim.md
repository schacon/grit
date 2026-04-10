# t2024-checkout-dwim

## Summary

Made `tests/t2024-checkout-dwim.sh` pass (23/23).

## Changes

- **Checkout DWIM**: Match Git `unique_tracking_name` by mapping `refs/heads/<branch>` through each remote's fetch refspecs (`map_ref_through_refspecs` / `remote_fetch_refspecs`), honoring `checkout.defaultRemote`, ambiguous-remote advice (`advice.checkoutAmbiguousRemoteBranchName`), file-vs-tracking disambiguation, and `checkout -p` when the arg is not a commit.
- **`--no-guess` / `checkout.guess=false`**: Thread `remote_branch_name_guess` through `resolve_revision_impl` so unique remote-tracking short names are not resolved as commits when guess is off (matches Git pathspec failure instead of detached HEAD).
- **`rev-parse`**: Accept `remotes/<remote>/<ref>` as shorthand for `refs/remotes/...` (tests use `remotes/repo_a/foo`).
- **Upstream display**: Normalize `branch.*.merge` when `remote = .` to `refs/heads/<name>` so loose `merge = main` compares correctly to `refs/heads/main` (t2024 loose vs strict message).

## Validation

- `./scripts/run-tests.sh t2024-checkout-dwim.sh`
- `cargo test -p grit-lib --lib`
