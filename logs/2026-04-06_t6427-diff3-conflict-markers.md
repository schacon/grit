## 2026-04-06 — t6427-diff3-conflict-markers

### Scope
- Complete `tests/t6427-diff3-conflict-markers.sh` from 3/9 to full pass.

### Reproduction
- Direct run:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6427-diff3-conflict-markers.sh`
  - Initial failures were in:
    - unique merge-base conflict marker base label
    - multiple merge-bases base label
    - rebase `--merge` / `--apply` base labels in conflict markers
    - zdiff3 marker shape and label ordering

### Implementation
- `grit/src/commands/merge.rs`
  - Added explicit base-label-prefix plumbing into `merge_trees`.
  - Added `resolve_conflict_labels(...)` to select:
    - ours label (`HEAD` or temporary merge branch label)
    - base label derived from merge-base context
    - style-sensitive formatting (`:content` retained for diff3 contexts, omitted for short OID in zdiff3).
  - Added conflict-style resolution from `merge.conflictstyle`.
  - Propagated base label and style to both normal and add/add content merge paths.

- `grit/src/commands/rebase.rs`
  - Added backend-aware rebase conflict context:
    - merge backend base label: `parent of <picked subject>`
    - apply backend base label: `constructed fake ancestor`
  - Persist backend marker in rebase state and reload during replay.
  - Threaded conflict context through rebase content merge.
  - On conflict, now records and writes merged conflict-marker content to worktree paths (instead of leaving plain stage-2 content), enabling grep-based validation in t6427.

- `grit-lib/src/merge_file.rs`
  - Added zealous-diff3 (`zdiff3`) shaping logic for insert-heavy conflicts:
    - detect insertion-only hunks around single-sided replacements,
    - convert to conflict hunks where appropriate,
    - compact shared prefix/suffix lines out of conflict blocks for expected zdiff3 output.
  - Added regression unit test:
    - `merge_file::tests::zdiff3_interesting_conflict_shape`
    - validates expected marker placement and preserved surrounding context.

### Validation
- `cargo build --release -p grit-rs` ✅
- Direct:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6427-diff3-conflict-markers.sh` ✅ 9/9
- Harness:
  - `./scripts/run-tests.sh t6427-diff3-conflict-markers.sh` ✅ 9/9
- Targeted regressions:
  - `./scripts/run-tests.sh t6404-recursive-merge.sh` ✅ 6/6
  - `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` ✅ 7/7
- Quality gates:
  - `cargo fmt` ✅
  - `cargo clippy --fix --allow-dirty` ✅ (unrelated edits reverted)
  - `cargo test -p grit-lib --lib` ✅ 97/97
