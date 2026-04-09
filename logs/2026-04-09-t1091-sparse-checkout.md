# t1091 sparse-checkout progress (2026-04-09)

## Changes

- **Cone parse:** Treat `!/*/` like Git when building `ConePatterns` (clear full-cone after `/*`).
- **`sparse-checkout add`:** Merge existing dirs via `cone_directory_inputs_for_add`; require cone-parseable file when `core.sparseCheckoutCone` before add (matches Git die message).
- **`apply_sparse_patterns`:** Skip index/worktree updates when `.git/index` is missing (`clone --no-checkout`).
- **`ensure_index_from_head_if_missing`:** No error on unborn HEAD.
- **Worktrees:** Write relative `commondir`; copy `config.worktree` into linked admin dir.
- **`rev-parse --git-path`:** Absolute path output; do not resolve `info/sparse-checkout` under commondir for linked worktrees.
- **`read-tree` / index sparse:** Use single `load_sparse_checkout_with_warnings` result for effective cone vs non-cone (emit “disabling cone pattern matching” warnings).
- **Unknown commands:** Dotted names skip autocorrect (stable `test_must_fail git … core.key`).
- **Lock message:** Quote path in “Unable to create … File exists” (t1091 grep).

## Test status

After changes: **26/77** pass in `t1091-sparse-checkout-builtin.sh` (1 expected TODO failure). Remaining: bare-repo check-rules, `ls-files`/sparse-index, warnings, clean, merge+sparse-index, submodules, clap unknown-option, etc.
