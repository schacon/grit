# t6030-bisect-porcelain (partial pass)

## Summary

Improved `git bisect` compatibility for harness `t6030-bisect-porcelain.sh`.

### Changes

- **BISECT_LOG**: append `git bisect start …` after `bisect_write` lines (avoid truncating appended log).
- **Terms**: align `check_term_format` with upstream C; fix `bisect terms` paragraph newline.
- **State commands**: route literal `bad`/`good`/`new`/`old` through `passive_state_cmd`; map synonyms in `bisect_write` for custom terms.
- **Replay**: tokenize replay lines like Git (`git bisect` + whitespace); handle `terms` subcommand; support CRLF.
- **Skip ranges**: expand `A..B` via `split_revision_token` + `rev_list` (positive/negative), not single-spec resolve.
- **bisect run**: redirect stdout to `BISECT_RUN` during state updates (dup2 via `nix`); detect cleared state via missing `BISECT_START`; match Git error for script deleting state.
- **Main**: bypass clap for `bisect` argv so unknown `--bisect-*` matches Git usage.
- **rev-parse**: `treeish:path` for rev-parse resolves directory leaves to subtree OID (`resolve_tree_path_rev_parse`); blob-at-path still uses blob-only walk.
- **Bisect**: recursive `verify_commit_tree_fully_readable` for checkout-mode start/next; `bisect visualize` uses `preprocess_log_argv_for_spawn` and passes user `--` pathspecs after bisect names.

### Test result

`./scripts/run-tests.sh t6030-bisect-porcelain.sh`: **84/96** passing at last run.

Remaining failures (12): path-restricted bisect / parallel skip chain, several `--no-checkout` broken-tree scenarios, ambiguous `bisect run`, skip-only log ordering, `bad HEAD` on parallel branch, custom-term skip, visualize with dash+space path.

## Reason stopped

`blocked` on remaining edge cases (pathspec/OR semantics vs Git for multi-path bisect, no-checkout broken trees, bisect run ambiguity) without further scope in this iteration.
