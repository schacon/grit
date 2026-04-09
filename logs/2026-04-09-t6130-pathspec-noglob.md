# t6130-pathspec-noglob

- **Root cause:** `git log` path filtering used `crate::pathspec::pathspec_matches`, which ignores `GIT_*_PATHSPECS`, `:(glob)` / `:(literal)`, and Git’s `simple_length` + `wildmatch` tail semantics.
- **Fix:** `log.rs` now uses `grit_lib::pathspec::matches_pathspec`. `grit-lib` `matches_pathspec_with_context` delegates to the same magic-aware tail matcher as `pathspec_matches`; `pathspec_matches_tail` matches Git’s literal-prefix + wildmatch-rest behavior.
- **wildmatch:** Reworked `dowild` loop increments to mirror Git’s C `for` (bracket handling + `*` + `/` with `WM_PATHNAME`).
- **Tests:** `./scripts/run-tests.sh t6130-pathspec-noglob.sh` → 21/21; `cargo test -p grit-lib --lib`.
