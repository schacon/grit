## Task
- Continue `t5516-fetch-push` parity work from current phase-6 target.
- Focus this slice on push parser compatibility cases around `--no-force` and `tag <name>` shorthand.

## Investigation
- Baseline before this change: `t5516-fetch-push` at **60/124**.
- Failing block showed:
  - `git push --no-force ...` errored as unknown option previously.
  - `git push ../child2 tag testTag` was interpreted as rev `tag` + path/revision parsing, causing:
    - `fatal: ambiguous argument 'tag'` (initial attempt), then
    - `src refspec 'refs/tags/tag' does not match any` when shorthand expansion was applied at the wrong stage.
- Root cause:
  - `--no-force` not modeled in push args.
  - `tag <name>` parsing needed to happen while iterating explicit refspec tokens, not in a single pre-parse helper.

## Code changes
- `grit/src/commands/push.rs`
  - Added hidden compatibility flag:
    - `#[arg(long = "no-force", hide = true)] pub no_force: bool`
  - Introduced effective force computation:
    - `let cli_force_enabled = args.force && !args.no_force;`
  - Replaced relevant force checks/output forced markers to use `cli_force_enabled` instead of raw `args.force`.
  - In explicit-refspec loop, added `tag <name>` pair handling:
    - token sequence `tag testTag` now expands to:
      - `src = refs/tags/testTag`
      - `dst = refs/tags/testTag`
    - keeps compatibility with Git’s shorthand syntax for push.
  - Added/kept aliased-ref consistency guard:
    - detect symbolic-ref destination aliases on remote;
    - reject inconsistent dual updates with:
      - `refusing inconsistent update between symref ... and its target ...`
    - drop redundant identical alias updates to avoid writing through symref as direct ref.

## Validation
- Quality gates:
  - `cargo fmt` ✅
  - `cargo check` ✅
  - `cargo clippy --fix --allow-dirty` ✅ (reverted unrelated edits)
  - `cargo test -p grit-lib --lib` ✅
- Target suite:
  - `./scripts/run-tests.sh t5516-fetch-push.sh` → **62/124** ✅ (from 60/124)
- Verbose confirmation:
  - previous failures `not ok 85` and `not ok 86` no longer present.
  - next remaining failures continue in later sections (e.g. 87+ fetch tag clobbering behavior, push porcelain, negotiation, etc.).

## Result
- Net improvement in `t5516-fetch-push`: **+2 passing tests** this slice.
- Parser compatibility for `push --no-force` and `push <remote> tag <name>` now matches expected test behavior.
