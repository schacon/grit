## t4057-diff-combined-paths — paths deleted from merge result

- **Failure:** Test 4 (`merge removed a file`) — `git diff -c --name-only HEAD HEAD^ HEAD^2` expected `4.txt`, grit printed nothing.
- **Cause:** `combined_diff_paths` intersected per-parent changed paths correctly, but `paths_in_tree_order` only walked the **merge** tree. Paths absent from the merge (removed vs all parents) never appeared in the walk.
- **Fix:** After the merge-tree-ordered pass, append any remaining paths from the intersection (sorted) so deleted-at-merge paths are listed like Git.

Validation: `./scripts/run-tests.sh t4057-diff-combined-paths.sh` (4/4), `cargo test -p grit-lib --lib`.
