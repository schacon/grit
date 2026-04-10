## 2026-04-10 — status/fsmonitor interface parity (phase 4 partial)

### Scope

Implemented the missing interface pieces required by `t7519-status-fsmonitor.sh` to move beyond immediate CLI/helper failures:

- index `FSMN` extension token read/write support,
- `update-index` fsmonitor switches,
- `ls-files -f` support,
- `test-tool dump-fsmonitor`.

This was intentionally constrained to interface compatibility and did not yet implement hook-driven invalidation semantics in `status`.

### Code changes

1. `grit-lib/src/index.rs`
   - Added `INDEX_EXT_FSMONITOR` signature handling.
   - Added `Index::fsmonitor_last_update: Option<String>`.
   - Parse `FSMN` extension token in `Index::parse`.
   - Serialize token back as `FSMN` extension in `serialize_into`.
   - Added `IndexEntry` fsmonitor-valid helpers using the CE flag bit:
     - `fsmonitor_valid()`
     - `set_fsmonitor_valid(bool)`

2. `grit/src/commands/update_index.rs`
   - Added support for:
     - `--fsmonitor`
     - `--no-fsmonitor`
     - `--fsmonitor-valid <path>...`
     - `--no-fsmonitor-valid <path>...`
     - `--force-write-index`
   - Added bare/virtual repository compatibility checks for fsmonitor options.
   - Added token enable/disable behavior (`builtin:fake` token for now).
   - Added per-entry fsmonitor-valid bit toggling for provided paths.
   - Extended sticky arg skipping so fsmonitor-valid options are parsed correctly in mixed argument streams.

3. `grit/src/commands/ls_files.rs`
   - Added `-f` option (`show_fsmonitor_valid_tag`), matching Git’s lowercase tag behavior for fsmonitor-valid entries.

4. `grit/src/main.rs`
   - Added `test-tool dump-fsmonitor` command output:
     - `no fsmonitor`
     - `fsmonitor last update <token>`

5. `grit/src/commands/add.rs`
   - New stage-0 entries now start with fsmonitor-valid cleared to avoid stale valid bits after restaging.

### Validation

- `cargo fmt`
- `cargo check -p grit-rs`
- `cargo build --release -p grit-rs`
- Targeted smoke check:
  - `update-index --fsmonitor`
  - `update-index --fsmonitor-valid ...`
  - `ls-files -f`
  - `test-tool dump-fsmonitor`
- Harness:
  - `./scripts/run-tests.sh t7519-status-fsmonitor.sh` → **12/33** (up from 8/33 baseline)

### Current gap

Remaining failures are predominantly behavior parity (hook token processing, selective invalidation, and full status/fsmonitor interaction), plus test-environment interactions where `.gitconfig` appears in index output in this harness setup.
