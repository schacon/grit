# t3428-rebase-signoff

- Added `test_commit_message` to `tests/test-lib.sh` (tests were failing with "command not found").
- Rebase: skip preemptive fast-forward when `--signoff` is set (Git uses REBASE_FORCE).
- `--root` without `--onto`: synthetic empty root commit; `--keep-empty`; optional patch-id dedup skip for implicit root.
- Merge backend: no parent-OID fast-path (conflicts); `CHERRY_PICK_HEAD` + `# Conflicts:` in `MERGE_MSG`; merge `--continue` runs `grit commit --cleanup=strip` (editor for t3428).
- `commit`: `--no-edit` skips editor; `--cleanup=strip` strips `#` lines when reading `MERGE_MSG`.
- Interactive rebase: parse `pick`/`edit`; stop after `edit` with `awaiting_amend`; `rebase --continue` resumes; todo lines `verb oid`.
- `rebase --continue`: after successful pick, pop first todo line (fix duplicate apply).
- `pull.rs`: `keep_empty: false` on `rebase::Args`.

Harness: `./scripts/run-tests.sh t3428-rebase-signoff.sh` → 7/7.
