## Task
- Continue phase-6 `t5516-fetch-push` parity work after negotiation and URL rewrite slices.
- Focus this slice on push refspec destination ambiguity/validation behavior.

## Investigation
- Current failing cluster in `t5516` included:
  - `not ok 26`/`30` (destination ambiguity handling),
  - `not ok 31` (`HEAD:refs/onelevel` should fail),
  - `not ok 38`/`39`/`40` (incomplete destination and invalid src combinations should fail).
- Root causes:
  1. Destination resolution was too simplistic (`normalize_ref` to `refs/heads/*`) and did not inspect remote namespaces for ambiguity.
  2. One-level full refs like `refs/onelevel` were accepted; Git rejects these as invalid full refnames in push destination context.
  3. Source DWIM was too strict for cases where destination was explicitly short/incomplete and should prefer branch source resolution.

## Code changes
- Updated `grit/src/commands/push.rs`:
  1. Added destination resolver:
     - `resolve_destination_ref_for_push(remote_git_dir, dst, local_ref) -> Result<String>`
     - enforces destination validation using `grit_lib::check_ref_format::check_refname_format`
     - rejects invalid full refnames with:
       - `The destination you provided is not a full refname`
     - resolves short/incomplete destinations by checking existing remote refs under:
       - `refs/heads/*`, `refs/tags/*`, `refs/remotes/*`
     - errors on ambiguity with Git-style:
       - `error: dst refspec <dst> matches more than one`
       - `failed to push some refs`
  2. Updated explicit CLI refspec and config-driven refspec paths to use the new destination resolver.
  3. Extended source resolver signature:
     - `resolve_push_src_for_refspec(repo, src, dst)`
     - when source DWIM has multiple matches and destination is short/non-full, prefer `refs/heads/<src>` (aligns with Git behavior for `main:origin/main`-style pushes).

## Validation
- Build gates:
  - `cargo fmt` ✅
  - `cargo check -p grit-rs` ✅
  - `cargo build --release -p grit-rs` ✅
- Targeted tests:
  - `bash tests/t5516-fetch-push.sh --run=1,31,32,38,39,40` ✅ all selected pass.
  - `bash tests/t5516-fetch-push.sh --run=1,25-40` improved cluster parity and no regressions in neighboring refspec tests.
- Suite:
  - `./scripts/run-tests.sh t5516-fetch-push.sh` → **68/124** (up from 66/124).

## Result
- Refspec destination parsing/validation now better matches Git for ambiguous/incomplete/full-ref destination cases.
- `t5516` moved from **66/124 → 68/124** in this slice.
