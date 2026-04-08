# t3417-rebase-whitespace-fix

- Added `grit_lib::whitespace_rule` (`parse_whitespace_rule`, `ws_fix_copy`, `fix_blob_bytes` with EOF blank trim).
- `rebase --whitespace=fix|strip`: no preemptive FF; store action in rebase state; merge base = HEAD tree during fix replay; post-pass `fix_blob_bytes` on merged index.
- `diff`: `grit diff <rev>:<path> <file>` for `git diff --exit-code HEAD^:file expect` style checks.
- Harness: `./scripts/run-tests.sh t3417-rebase-whitespace-fix.sh` → 4/4.
- Push to `origin` failed in agent env (remote not writable).
