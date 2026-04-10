# Test results

**2026-04-10 (t4252 / am apply passthrough options)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t4252-am-options.sh`: 8/8 passed

**2026-04-10 (t3438 / rebase broken files)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t3438-rebase-broken-files.sh`: 9/9 passed

**2026-04-10 (t3405 / rebase malformed messages)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t3405-rebase-malformed.sh`: 5/5 passed

**2026-04-10 (t3416 / rebase --onto A...B and --keep-base)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t3416-rebase-onto-threedots.sh`: 18/18 passed

**2026-04-10 (t5581 / GIT_CURL_VERBOSE)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5581-http-curl-verbose.sh`: 2/2 passed

**2026-04-10 (t0012-help)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t0012-help.sh`: 121/121 passed

**2026-04-10 (t5528 / push.default)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5528-push-default.sh`: 31/32 passed (1 `test_expect_failure`)

**2026-04-10 (t5327 / multi-pack bitmaps .rev)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5327-multi-pack-bitmaps-rev.sh`: 314/314 passed (expected after merge)

**2026-04-10 (t3452 / history split)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t3452-history-split.sh`: 25/25 passed

**2026-04-10 (t5532 / fetch proxy)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5532-fetch-proxy.sh`: 5/5 passed

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

**2026-04-10 (t8008 / blame formats)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t8008-blame-formats.sh`: 5/5 passed

**2026-04-10 (t0035 / safe.bareRepository)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t0035-safe-bare-repository.sh`: 12/12 passed

**2026-04-09 (t5410 / receive-pack)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5410-receive-pack.sh`: 5/5 passed

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
