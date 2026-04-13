## 2026-04-10 — phase4: guard fsmonitor refresh query on hook-style config only

### Context

After integrating fsmonitor refresh invalidation, `update-index --refresh` began consulting
fsmonitor metadata whenever the index had an `FSMN` token. That is valid for hook-based
`core.fsmonitor=<path>` configurations, but not for plain boolean config forms.

In mixed harness states this could make refresh behavior over-eager and interfere with
expected fsmonitor-valid bit transitions in status-related tests.

### Change

- Tightened `update-index --refresh` fsmonitor query gating:
  - only query the fsmonitor hook when:
    - `index.fsmonitor_last_update` is present, and
    - `core.fsmonitor` is configured to a **hook path value** (not boolean on/off).
- Kept existing token-update and reported-path filtering behavior unchanged for valid hook mode.

### Validation

- `cargo fmt`
- `cargo check -p grit-rs`
- `cargo build --release -p grit-rs`
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh` → **22/33**
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh` → **14/58**
- `./scripts/run-tests.sh t7508-status.sh` → **94/126**
- `./scripts/run-tests.sh t7060-wtstatus.sh` → **12/17**
- `./scripts/run-tests.sh t7065-status-rename.sh` → **28/28**

### Notes

- This increment improves fsmonitor/status parity while avoiding invalid hook queries in
  non-hook config states.
- Remaining `t7519` failures are now concentrated in deeper fsmonitor-valid bit and
  sparse/UNTR interactions rather than setup/bootstrap behavior.
