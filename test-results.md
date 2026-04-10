# Test results

**2026-04-10 (status perf phase4 / refresh reported-path handling)**

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `bash tests/t7519-status-fsmonitor.sh -v`: 27/33 (`7519.12` now passing; remaining fails: 13,20,27,30,31,33)
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 22/33
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 14/58 (no regression)
- `./scripts/run-tests.sh t7508-status.sh`: 94/126 (no regression)
- `./scripts/run-tests.sh t7060-wtstatus.sh`: 12/17 (no regression)
- `./scripts/run-tests.sh t7065-status-rename.sh`: 28/28 (no regression)

**2026-04-10 (status perf phase4 / fsmonitor refresh hook applies only to --refresh path)**

- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `bash tests/t7519-status-fsmonitor.sh --run=12 -v` (with harness env): still fails (`test_must_fail git update-index --refresh --force-write-index` unexpectedly succeeded)
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 22/33 (improved from 18/33)
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 14/58 (no change)
- `./scripts/run-tests.sh t7508-status.sh`: 94/126 (holds improved baseline)
- `./scripts/run-tests.sh t7060-wtstatus.sh`: 12/17 (holds improved baseline)
- `./scripts/run-tests.sh t7065-status-rename.sh`: 28/28

**2026-04-10 (status perf phase4 / status fsmonitor query integration)**

- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 18/33 (no regression)
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 14/58 (no regression)
- `./scripts/run-tests.sh t7508-status.sh`: 94/126 (improved from 48/126)
- `./scripts/run-tests.sh t7060-wtstatus.sh`: 12/17 (improved from 10/17)
- `./scripts/run-tests.sh t7065-status-rename.sh`: 28/28 (improved from 27/28)

**2026-04-10 (status perf phase4 / fsmonitor refresh hook + add semantics fix)**

- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 18/33 (from 12/33)
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 14/58 (from 12/58)

**2026-04-10 (status perf phase4 / remove add-side auto root commit)**

- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `bash tests/t7519-status-fsmonitor.sh --run=4 -v` (with harness env): passed (`ok 4 - setup`)
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 18/33
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 14/58

**2026-04-10 (status perf phase4 / split-index option compatibility)**

- `cargo check -p grit-rs`: passed
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 12/33

**2026-04-10 (status perf phase3 / ignored-directory prune)**

- `cargo check -p grit-rs`: passed
- `cargo test -p grit-lib --lib`: 166 passed
- `./scripts/run-tests.sh t0008-ignores.sh`: 219/398
- `./scripts/run-tests.sh t7067-status-untracked-dir.sh`: 32/33
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 12/58
- `./scripts/run-tests.sh t7508-status.sh`: 48/126

**2026-04-10 (status perf phase2 / untracked cache collection fast-path)**

- `cargo check -p grit-rs`: passed
- `cargo test -p grit-lib --lib`: 166 passed
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 12/58
- `./scripts/run-tests.sh t7508-status.sh`: 48/126
- `./scripts/run-tests.sh t7060-wtstatus.sh`: 10/17
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 8/33

**2026-04-10 (status perf phase5 / rename detection budget)**

- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `./scripts/run-tests.sh t7065-status-rename.sh`: 27/28
- `./scripts/run-tests.sh t7508-status.sh`: 48/126

**2026-04-10 (t5705 / session ID in capabilities)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5705-session-id-in-capabilities.sh`: 17/17 passed

**2026-04-10 (t6101 / rev-parse parents)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t6101-rev-parse-parents.sh`: 38/38 passed

**2026-04-09 (t4103 / apply binary)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t4103-apply-binary.sh`: 24/24 passed

**2026-04-10 (t7413 / submodule is-active)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t7413-submodule-is-active.sh`: 10/10 passed

**2026-04-10 (t3702 / add -e)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t3702-add-edit.sh`: 3/3 passed

**2026-04-10 (t4122 / apply symlink inside)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t4122-apply-symlink-inside.sh`: 7/7 passed

**2026-04-10 (status perf phases 3/4 slice)**

- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `./scripts/run-tests.sh t7508-status.sh`: 48/126
- `./scripts/run-tests.sh t7060-wtstatus.sh`: 10/17
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 12/58
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 12/33

**2026-04-10 (t8008 / blame formats)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t8008-blame-formats.sh`: 5/5 passed

**2026-04-10 (t0035 / safe.bareRepository)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t0035-safe-bare-repository.sh`: 12/12 passed

**2026-04-09 (t4063 / diff blobs)**

- `cargo test -p grit-lib --lib`: 155 passed
- `./scripts/run-tests.sh t4063-diff-blobs.sh`: 18/18 passed

**2026-04-09 (t5571 / pre-push hook)**

- `cargo test -p grit-lib --lib`: 155 passed
- `./scripts/run-tests.sh t5571-pre-push-hook.sh`: 11/11 passed

**2026-04-09 (t7418 / submodule sparse .gitmodules)**

- `cargo test -p grit-lib --lib`: 155 passed
- `./scripts/run-tests.sh t7418-submodule-sparse-gitmodules.sh`: 9/9 passed

**2026-04-09 (t5609 / clone --branch)**

- `cargo test -p grit-lib --lib`: 152 passed
- `./scripts/run-tests.sh t5609-clone-branch.sh`: 7/7 passed

**2026-04-09 (t3422 / rebase incompatible options)**

- `cargo test -p grit-lib --lib`: 152 passed
- `./scripts/run-tests.sh t3422-rebase-incompatible-options.sh`: 52/52 passed

**2026-04-09 (t2203-add-intent)**

- `cargo test -p grit-lib --lib`: 152 passed
- `./scripts/run-tests.sh t2203-add-intent.sh`: 19/19 passed

**2026-04-09 (t3417 / rebase whitespace fix)**

- `cargo test -p grit-lib --lib`: 147 passed
- `./scripts/run-tests.sh t3417-rebase-whitespace-fix.sh`: 4/4 passed

**2026-04-09 (t5318 / pack-objects --revs)**

- `cargo test -p grit-lib --lib`: 147 passed
- `./scripts/run-tests.sh t5318-pack-objects-revs-exclude.sh`: 9/9 passed

**2026-04-10 (status perf follow-up: phase2 + phase4 interfaces)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 12/58
- `./scripts/run-tests.sh t7508-status.sh`: 48/126
- `./scripts/run-tests.sh t7060-wtstatus.sh`: 10/17
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 12/33

Notes:
- Phase2 optimization reuses populated UNTR cache for untracked-only status collection (`--ignored=no`) to remove one duplicate full tree walk.
- Phase4 interface work implemented missing fsmonitor command/test-tool surfaces needed by status-associated suites:
  `update-index --fsmonitor`, `--no-fsmonitor`, `--fsmonitor-valid`, `--no-fsmonitor-valid`, `--force-write-index`,
  `ls-files -f`, and `test-tool dump-fsmonitor`.
- `t7519` improved from 8/33 to 12/33; remaining failures are deeper behavior parity (hook-driven invalidation and status integration), tracked for subsequent phase work.
