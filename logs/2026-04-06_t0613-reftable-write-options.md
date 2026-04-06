## t0613-reftable-write-options (2026-04-06)

### Goal
Make `t0613-reftable-write-options.sh` fully pass and preserve nearby hook behavior.

### Root causes addressed
- Reftable write options were not fully aligned with Git behavior for config/env parsing and limits.
- Reftable writer block/index emission had mismatches in restart handling, block-length accounting, and object index metadata.
- Reftable update-index encoding/decoding semantics differed from Git (needed table-relative delta form).
- `update-ref --stdin` transaction updates in reftable mode needed stable monotonic update-index assignment across queued operations.
- `test-tool dump-reftable` output needed block-header decoding parity for upstream assertions.
- Regression discovered in `t1416`: `HEAD` symbolic resolution in reftable repos returned direct table records instead of symbolic `HEAD` file target for hook payload mapping.

### Key implementation changes
- `grit-lib/src/reftable.rs`
  - Hardened write option parsing and validation via config loaders and typed parsers.
  - Added/updated block and index writing logic (ref/log/object) including restart offsets, lengths, and footer positions.
  - Implemented table-relative update-index encoding and matching decode behavior.
  - Added write-path support for caller-provided update-index to support transactional sequencing.
  - Added normalized reflog message shaping for reftable log writes.
- `grit/src/commands/update_ref.rs`
  - Improved stdin batch transaction state handling and update-index assignment flow.
  - Ensured reftable transaction base index is derived once per transaction and incremented per queued op.
- `grit/src/main.rs`
  - Implemented `test-tool dump-reftable` compatible block decoding/printing.
- `grit/src/commands/pack_refs.rs`
  - Added reftable-aware compaction path with write-option handling/error mapping.
- `grit-lib/src/refs.rs`
  - Reftable `read_symbolic_ref("HEAD")` now reads `.git/HEAD` directly so symbolic HEAD semantics match hook expectations in reftable repos.
- `tests/test-lib.sh`
  - Kept compatibility helper updates needed by adjacent tests used during verification.

### Validation
- `cargo build --release -p grit-rs` ✅
- `GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash t1416-ref-transaction-hooks.sh` ✅ 10/10
- `./scripts/run-tests.sh t1416-ref-transaction-hooks.sh` ✅ 10/10
- `./scripts/run-tests.sh t0613-reftable-write-options.sh` ✅ 11/11
- `cargo fmt && cargo clippy --fix --allow-dirty && cargo test -p grit-lib --lib` ✅ (98/98)
