## 2026-04-06 — t6113-rev-list-bitmap-filters

### Baseline
- `./scripts/run-tests.sh t6113-rev-list-bitmap-filters.sh` initially reported `2/14`.
- Failures were concentrated in:
  - unsupported filter specs (`sparse:oid=...`, `object:type=...`)
  - unsupported option (`--filter-provided-objects`)
  - bitmap/filter interaction behavior checks
  - `--unpacked` object listing behavior

### Implementation changes

#### `grit-lib/src/rev_list.rs`
- Extended object filter model and parsing:
  - added `ObjectFilter::SparseOid(ObjectId)`
  - added `ObjectFilter::ObjectType(ExpectedObjectKind)` with support for `tag|commit|tree|blob`
  - added `ExpectedObjectKind::Tag` and parser `ExpectedObjectKind::from_str`
- Added filter compatibility policy:
  - `ObjectFilter::requires_non_bitmap_fallback()` now returns true for:
    - `sparse:oid`
    - `tree:<depth>` where depth > 0
    - any `object:type=...`
    - any `combine:` containing fallback-required subfilters
- Added `RevListOptions::filter_provided_objects` and threaded through traversal.
- Reworked object-root resolution:
  - `resolve_specs_for_objects` now returns:
    - traversal commits (`Vec<ObjectId>`)
    - explicit object roots (`Vec<RootObject>`) including display-name metadata
    - explicitly provided commit IDs (`HashSet<ObjectId>`)
  - explicit refs that resolve to tags now preserve tag-object output when required.
- Implemented provided-object filtering semantics:
  - explicit roots are filtered when `--filter-provided-objects` is set
  - explicit roots bypass filters when it is unset
- Added object traversal split:
  - keep display commit ordering list (`ordered`) for commit output
  - use separate `object_traversal_commits` for `--objects` walking, preserving traversal semantics expected by object filtering tests
- Added excluded-tip closure support for objects:
  - when negative revisions are present, object traversal excludes commits reachable from excluded tips.

#### `grit/src/commands/rev_list.rs`
- Added parsing for:
  - `--filter-provided-objects`
  - `--unpacked` (captured as behavior flag)
- Updated repeated `--filter=` handling to combine filters using helper:
  - `combine_object_filters(existing, incoming) -> ObjectFilter`
- Added bitmap-compatibility behavior:
  - `bitmap_traversal` display mode now disabled for incompatible filters using
    `requires_non_bitmap_fallback()`
  - this preserves path names in fallback scenarios.
- Added `--unpacked` post-processing:
  - filters object output to loose objects only (`repo.odb.object_path(oid).is_file()`)
  - suppresses commit output in unpacked object mode (commit set moved to object-only output path)

### Validation
- Direct:
  - `rm -rf tests/trash.t6113-rev-list-bitmap-filters tests/bin.t6113-rev-list-bitmap-filters && GUST_BIN=/workspace/target/release/grit bash tests/t6113-rev-list-bitmap-filters.sh`
  - result: **14/14 passing**
- Harness:
  - `./scripts/run-tests.sh t6113-rev-list-bitmap-filters.sh`
  - result: **14/14 passing**
- Regression checks:
  - `./scripts/run-tests.sh t6112-rev-list-filters-objects.sh` → `26/54` (pre-existing partial; no claim of completion)
  - `./scripts/run-tests.sh t6115-rev-list-du.sh` → `17/17` passing
  - `./scripts/run-tests.sh t6005-rev-list-count.sh` → `6/6` passing

### Notes
- During this task, repeated direct runs surfaced transient harness cwd/trash interference if runs were started from a previously deleted test cwd. Running from a stable workspace cwd and resetting the per-test trash/bin directories before direct run produced stable, conclusive results.
