# t12580-log-oneline-all

## Root causes

1. **`grit log --oneline` decorated by default** — `resolve_decoration_display` forced short decorations whenever `--oneline` was used. Real Git does not; output must match `git log --oneline` for t12580’s `test_cmp` checks. Removed that branch.

2. **Harness cwd after setup** — Many tests end setup with `grit init repo && cd repo && …` and leave the shell cwd inside `repo`. Later bodies use `(cd repo && … >../actual)`; `..` is resolved relative to the *current* directory before `cd`, so from inside `repo`, `../actual` pointed outside the trash and `cd repo` failed. `test_eval_` now `cd`s to `TRASH_DIRECTORY` before and after each evaluated test body so `../` paths stay correct.

## Validation

- `./scripts/run-tests.sh t12580-log-oneline-all.sh` — 31/31
- `./scripts/run-tests.sh t13180-log-patch-stat.sh` — 35/35
