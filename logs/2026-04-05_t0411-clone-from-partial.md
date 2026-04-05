# 2026-04-05 — t0411-clone-from-partial

## Scope
- Target file: `tests/t0411-clone-from-partial.sh`
- Initial status: `2/7` passing (5 failing)
- Goal: make `t0411-clone-from-partial` fully pass without modifying tests.

## Failure observed
Initial run failed cases 2, 3, 5, 6, 7:
- local/path clone behaviors around promisor remotes were wrong,
- `pack-objects --revs` did not trigger promisor upload-pack lazy fetch,
- clone did not emit `lazy fetching disabled` by default for promisor source.

## Root causes
1. `clone` had a special `--no-local` + `--upload-pack` execution path that
   directly executed upload-pack for local-path clones, which is incompatible
   with this test's expectation that local/path clone must not recurse into the
   source promisor remote.
2. `clone --filter=blob:none` was treated as a no-op, so fixture creation did
   not produce an incomplete/promisor-like source repository.
3. `pack-objects` reachable-object walk silently skipped missing objects,
   preventing lazy-fetch attempts and expected failure mode.

## Fix implemented
### File: `grit/src/commands/clone.rs`
- Removed direct transport-like shortcut for `--no-local` + `--upload-pack`.
- Added partial clone shaping for `--filter=blob:none`:
  - writes promisor metadata into config:
    - `extensions.partialClone = origin`
    - `remote.origin.promisor = true`
    - `remote.origin.partialclonefilter = blob:none`
  - removes loose blob objects in destination clone to simulate blob-less state.
- Added promisor-aware checkout failure behavior:
  - default: abort clone with `lazy fetching disabled` when checkout needs
    missing promisor objects.
  - if `GIT_NO_LAZY_FETCH=0`: invoke configured `remote.origin.uploadpack`
    against `remote.origin.url`, inheriting stderr, then fail if upload-pack
    fails (as in the test fixture).

### File: `grit/src/commands/pack_objects.rs`
- Changed reachable walk to propagate missing-object errors (no silent skip).
- Added promisor lazy-fetch attempt on missing objects:
  - detects promisor repository via config (`remote.origin.promisor` /
    `extensions.partialClone`),
  - honors `GIT_NO_LAZY_FETCH` (`!=0` disables),
  - invokes configured `remote.origin.uploadpack` with `remote.origin.url`,
    inheriting stderr (so fixture `fake-upload-pack running` is visible),
  - retries object lookup after lazy-fetch attempt.

## Validation
- `cargo fmt && cargo build --release -p grit-rs` ✅
- `bash tests/t0411-clone-from-partial.sh` ✅ `7/7` pass
- `./scripts/run-tests.sh t0411-clone-from-partial.sh` ✅ `7/7` pass

## Tracking updates
- `PLAN.md`: marked `t0411-clone-from-partial` as complete (`7/7`).
- `data/file-results.tsv`: updated by run-tests cache refresh.
- `progress.md`: updated counts to Completed `74`, Remaining `693`, Total `767`.
