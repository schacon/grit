## t4067-diff-partial-clone (claim/baseline)

### Claim

- Claimed next Diff target from plan: `t4067-diff-partial-clone`.
- Updated plan status from `[ ]` to `[~]`.

### Baseline

- `./scripts/run-tests.sh t4067-diff-partial-clone.sh` -> 0/9 passing.
- `bash scripts/run-upstream-tests.sh t4067-diff-partial-clone` -> 2/9 passing.

### Initial failing assertions snapshot (upstream)

- 1: `git show` on partial-clone bare client should batch missing blob fetches into one negotiation.
- 2: `git diff HEAD^ HEAD` should batch missing blob fetches into one negotiation.
- 3: `git diff` should avoid fetching unchanged same-OID blobs.
- 4: `git diff` should skip fetching gitlink/submodule pseudo-blobs.
- 5: `git diff --raw -M` should batch rename-detection blob prefetch.
- 8: exact-rename case should avoid any fetch when inexact detection is unnecessary.
- 9: `--break-rewrites -M` should fetch only when required and batch when it does.
