# 2026-04-06 — t1511-rev-parse-caret

## Scope
- Target file: `tests/t1511-rev-parse-caret.sh`
- Start: `10/17` passing
- Goal: full pass without modifying upstream tests

## Failures addressed
1. `^{tag}` peel operator unsupported.
2. `^{/pattern}` commit-message search inside peel syntax unsupported.
3. `^{/!!pattern}` literal-`!` escaping unsupported.
4. `^{/!-pattern}` negative message search unsupported.

## Implemented fixes

### `grit-lib/src/rev_parse.rs`
- Extended `apply_peel` with:
  - `Some("tag")`: peel to tag object and verify object is a tag.
  - `Some(op)` where `op` starts with `/`: commit-message search relative to the peeled commit:
    - `/.` => current commit
    - `/pattern` => first reachable commit whose message contains `pattern`
    - `/!!pattern` => match literal `!pattern`
    - `/!-pattern` => first reachable commit whose message does **not** contain `pattern`
    - `/!-!pattern` => negative search with literal `!pattern`
- Added helper:
  - `resolve_commit_message_search_from(repo, start_oid, pattern)` to perform BFS message search from an explicit start commit.

## Validation
- `cargo build --release -p grit-rs` ✅
- `rm -rf /workspace/tests/trash.t1511-rev-parse-caret && GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash tests/t1511-rev-parse-caret.sh` ✅ `17/17`
- `./scripts/run-tests.sh t1511-rev-parse-caret.sh` ✅ `17/17`

## Tracking updates
- `PLAN.md`: marked `t1511-rev-parse-caret` complete (`17/17`).
- `progress.md`: updated counts to Completed `83`, Remaining `684`, Total `767`; added `t1511` to recent completions.
- `test-results.md`: prepended `t1511` build/test evidence.
