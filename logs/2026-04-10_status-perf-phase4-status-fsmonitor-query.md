## 2026-04-10 — phase4: status fsmonitor query integration

### Context

Status-associated fsmonitor tests still had a major gap: `status` did not actually
query the configured fsmonitor hook and therefore did not limit changed-path checks.

The plan explicitly calls for status-side fsmonitor behavior, so this increment
implemented that path in `status` itself.

### Change

- `grit/src/commands/status.rs`
  - Added fsmonitor hook payload parser and status-specific hook query helper:
    - `parse_fsmonitor_payload`
    - `query_status_fsmonitor_paths`
    - `fsmonitor_reported_path_matches`
  - During `status` execution, if fsmonitor is configured:
    - query hook with `2 <last_update_token>`;
    - update in-memory `index.fsmonitor_last_update` token;
    - filter unstaged diff entries to reported paths;
    - when untracked-cache is enabled, filter untracked/ignored output to reported paths.
  - Emits trace2 region `fsm_hook/query` for `GIT_TRACE2_EVENT` parity checks.

### Validation

- `cargo fmt`
- `cargo check -p grit-rs`
- `cargo build --release -p grit-rs`
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh` → 18/33 (no regression)
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh` → 14/58 (no regression)
- `./scripts/run-tests.sh t7508-status.sh` → 94/126 (improved from 48/126)
- `./scripts/run-tests.sh t7060-wtstatus.sh` → 12/17
- `./scripts/run-tests.sh t7065-status-rename.sh` → 28/28

### Notes

- This is an incremental parity step; remaining fsmonitor failures still include
  `ls-files -f` expectations around `.gitconfig` and deeper refresh/valid-bit semantics.
- The status command now has an explicit fsmonitor query path, which was absent before.
