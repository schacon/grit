## t4074-diff-shifted-matched-group

### Problem
`grit diff --no-index --histogram` omitted `diff --git` / `index` headers and used Myers line diff, so shifted/histogram hunks and `--ignore-all-space` output did not match Git.

### Fix
- **`grit/src/commands/diff.rs`**: For file `--no-index`, emit `diff --git` and `index <short>..<short> <mode>` (blob hash via `Odb::hash_object_data`). Resolve effective line algorithm from argv order (`--histogram` / `--patience` → `similar::Patience`, `--minimal` → Myers, `--diff-algorithm=`). Build line slots with byte-normalised compare keys (merge-style rules) and original bytes for output; emit hunks manually so context uses the **new** file’s text when whitespace is ignored (matches Git’s ` b` vs `b`).
- **`grit-lib/src/diff.rs`**: `anchored_unified_diff` takes `similar::Algorithm` and uses it for segment diffs (and fallback unified diff), so `--anchored=c --histogram` vs `--histogram --anchored=c` matches Git (t4065).
- **`grit/src/commands/show.rs`**: Pass Myers vs Patience into `anchored_unified_diff` when `--patience` is set.

### Validation
- `t4074-diff-shifted-matched-group.sh` 4/4
- `t4065-diff-anchored.sh` 7/7
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t4074-diff-shifted-matched-group.sh`
