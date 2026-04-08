# t5403-post-checkout-hook

## Goal

Make `tests/t5403-post-checkout-hook.sh` pass (14/14).

## Changes

- **`grit/src/commands/checkout.rs`**
  - Added `run_post_checkout_hook` using `grit_lib::hooks::run_hook` with args `<old-hex> <new-hex> <0|1>`.
  - Branch switches: `switch_branch`, `create_and_switch_branch`, `force_create_and_switch_branch`, `detach_head_inner` — run hook after HEAD update; special case “already on branch” with same commit still runs hook (Git behavior).
  - Path checkout: `checkout_paths` — flag `0`, old=new=current HEAD commit.
- **`grit/src/commands/rebase.rs`**
  - After initial checkout onto `onto` (full rebase and fast-forward rebase), run `post-checkout` with old=head before rewind, new=onto, flag `1`.
- **`grit/src/commands/clone.rs`**
  - `determine_head_branch`: map detached HEAD OID to `refs/heads/*` tip when unique; else pick branch like Git (`GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME`, main/master heuristic).
  - `default_initial_branch_for_clone` for init fallback instead of hardcoded `master`.
  - After `checkout_head`, run clone `post-checkout` (null old OID, new HEAD, `1`).
  - SSH clone path updated similarly.

## Validation

- `cargo fmt`, `cargo clippy -p grit-rs --fix --allow-dirty`
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t5403-post-checkout-hook.sh` → 14/14
