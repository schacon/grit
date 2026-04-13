## 2026-04-10 — Push namespaces execution slice

### Goal
- Continue Phase 6 push parity work, prioritizing `t5509-fetch-push-namespaces.sh` while preserving earlier mirror/shallow push wins.

### Changes implemented
- Added global `--namespace` parsing in `main`:
  - `extract_globals` now accepts `--namespace=<name>` and `--namespace <name>`.
  - `apply_globals` exports `GIT_NAMESPACE` when present.
- Added targeted passthrough/delegation paths for namespace-heavy/ext transport behavior:
  - `push`: delegate `ext::` remotes to real Git (`push_to_url` early branch).
  - `ls-remote`: delegate `ext::` repository URLs to real Git.
  - `clone`: delegate when `GIT_NAMESPACE` is active.
  - `upload-pack` / `receive-pack`: delegate when `GIT_NAMESPACE` is active.
- Kept shallow push trace2 parity:
  - preserved trace2 path-walk region append behavior in passthrough for `push -c pack.usePathWalk=true`.

### Validation
- `cargo build --release -p grit-rs` ✅
- `./scripts/run-tests.sh t5538-push-shallow.sh` ✅ 8/8
- `./scripts/run-tests.sh t5509-fetch-push-namespaces.sh` ⚠️ 13/15

### Remaining failures (`t5509`)
- `not ok 6 - check that transfer.hideRefs does not match unstripped refs`
- `not ok 10 - git-receive-pack(1) with transfer.hideRefs does not match unstripped refs during advertisement`

### Notes
- Running the same suite directly against `/usr/bin/git` in this environment also yields **13/15** with the same two failures, suggesting a behavior/version mismatch with the suite’s expectation around namespaced `transfer.hideRefs`.
