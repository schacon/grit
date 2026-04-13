## Task
- Continue PLAN execution on `t5510-fetch.sh` fetch parity.
- Focus this slice on CLI option parity and prune/followRemoteHEAD behavior.

## Changes made

### 1) Fetch CLI option parsing + behavior
- File: `grit/src/commands/fetch.rs`
- Added fetch args:
  - `--atomic`
  - `--append`
  - `--dry-run`
  - `--write-fetch-head`
  - `--no-write-fetch-head`
  - `--no-prune`
  - `--refmap <REFSPEC>` (repeatable, supports empty `--refmap=""`)
  - `-t` short alias for `--tags`
- Added behaviors:
  - `--no-prune` overrides prune-on command line.
  - `--refmap` requires command-line refspecs (`git fetch --refmap=... <remote> <refspec>`).
  - `FETCH_HEAD` writing honors:
    - `--dry-run` (no file write)
    - `--no-write-fetch-head` (no file write)
    - `--write-fetch-head` (explicit write)
    - `--append` (append mode rather than overwrite)
  - For `--dry-run`, emit `would write to .git/FETCH_HEAD` to stderr unless suppressed by `--no-write-fetch-head`.

### 2) Refmap tracking/prune integration
- File: `grit/src/commands/fetch.rs`
- Added explicit refmap parsing into `FetchRefspec` list for mapping source-only CLI refspecs to local tracking destinations.
- Ensured CLI updates contribute to `updated_refs` so prune logic does not incorrectly delete refs updated in same fetch.
- Prune namespace logic now:
  - Uses explicit CLI `<src>:<dst>` namespaces when provided.
  - Uses `--refmap` namespaces when explicit refmap is passed.
  - Uses configured `remote.<name>.fetch` namespaces otherwise.
  - Skips prune entirely for source-only CLI refspecs (e.g. `git fetch --prune origin main`) to match Git behavior.

### 3) Prune/pruneTags config-driven semantics
- File: `grit/src/commands/fetch.rs`
- Implemented effective prune flags from config:
  - `remote.<name>.prune` over `fetch.prune`
  - `remote.<name>.pruneTags` over `fetch.pruneTags`
- `pruneTags` only active if pruning is active, matching upstream fetch matrix behavior.

### 4) followRemoteHEAD trace parity fix for protocol-v2 upload-pack
- Files:
  - `grit/src/fetch_transport.rs`
  - `grit/src/commands/fetch.rs`
  - `grit/src/commands/clone.rs`
- Added a new boolean parameter to `fetch_via_upload_pack_skipping` / `v2_ls_refs_for_fetch` controlling whether to send `ref-prefix HEAD`.
- In fetch command path:
  - Pass `include_head_ref_prefix = (followRemoteHEAD != never)`.
- In clone path:
  - Always pass `true` (clone still needs HEAD symbolic target handling).
- Goal: stop emitting `fetch> ref-prefix HEAD` when `remote.<name>.followRemoteHEAD=never`.

## Test/validation runs

### Build/unit checks
- `cargo fmt`
- `cargo check -p grit-rs`
- `cargo test -p grit-lib --lib`
- `cargo build --release -p grit-rs`

All passed on this slice after code updates.

### Focused fetch tests
- Repeated targeted runs of:
  - `tests/t5510-fetch.sh --run=27..39,58..71,...`
  - `tests/t5510-fetch.sh --run=94,95,122,123,...,185`
  - `tests/t5510-fetch.sh --run=8,19,21,23,26`
- Outcomes:
  - Option parsing failures for `--atomic`, `--refmap`, `--dry-run`, `-t`, `--no-prune` removed.
  - `--no-write-fetch-head` and `--write-fetch-head` dry-run precedence tests now pass.
  - Several prune matrix cases improved after config-driven prune logic.
  - Remaining failures persist in prune+branch/name scenarios and broader non-prune areas.

### Full harness checkpoints
- `./scripts/run-tests.sh t5510-fetch.sh`
  - improved from prior baseline around **119/215** to **134/215**, then to **158/215**, then **161/215** after followRemoteHEAD gating and related fixes.

## Commits in this slice
- `d404ce84` — `feat(fetch): add option parsing for refmap dry-run and no-prune`
- `6971de3d` — `fix(fetch): include cli-updated refs in prune tracking`
- `c0bd8212` — `fix(fetch): honor configured prune and pruneTags defaults`
- `90696e38` — `fix(fetch): gate v2 HEAD ls-refs prefix for followRemoteHEAD`

## Remaining notable failures (post-slice)
- `t5510` still failing in areas including:
  - prune with branch/refspec overlap in some scenarios
  - atomic transaction semantics edge cases
  - bundle-related tests
  - follow-on connectivity/negotiation-tip and assorted advanced behaviors
- These map to remaining PLAN work beyond this incremental slice.
