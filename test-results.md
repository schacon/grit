# Test results

**2026-04-10 (phase 6 / t5516 fetch url.insteadOf parity)**  

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo clippy --fix --allow-dirty -p grit-rs`: pass (reverted unrelated formatting-only edits before commit)
- `cargo test -p grit-lib --lib`: pass
- `./scripts/run-tests.sh t5516-fetch-push.sh`: **59/124** (up from 58/124; `fetch with insteadOf` now passes via `url.<base>.insteadOf` rewrite on fetch URLs)

**2026-04-10 (phase 6 / t5516 no-ipv option parse parity)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo clippy --fix --allow-dirty -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: pass
- `./scripts/run-tests.sh t5516-fetch-push.sh`: **58/124** (up from 54/124 after matching git-style unknown-option handling for `push/fetch --no-ipv4|--no-ipv6`)

**2026-04-10 (phase 6 / embedded-gitlink checkout preservation fix)**

- `cargo build --release -p grit-rs`: pass
- `./scripts/run-tests.sh t5531-deep-submodule-push.sh`: 29/29
- `./scripts/run-tests.sh t5517-push-mirror.sh`: 13/13
- `./scripts/run-tests.sh t5538-push-shallow.sh`: 8/8
- `./scripts/run-tests.sh t5545-push-options.sh`: 13/13
- `./scripts/run-tests.sh t5509-fetch-push-namespaces.sh`: 13/15

**2026-04-10 (phase 6 / recurse-submodule parity delegation follow-up)**

- `cargo build --release -p grit-rs`: pass
- `./scripts/run-tests.sh t5517-push-mirror.sh`: 13/13
- `./scripts/run-tests.sh t5538-push-shallow.sh`: 8/8
- `./scripts/run-tests.sh t5545-push-options.sh`: 13/13
- `./scripts/run-tests.sh t5509-fetch-push-namespaces.sh`: 13/15
- `./scripts/run-tests.sh t5531-deep-submodule-push.sh`: 18/29

**2026-04-10 (phase 6 / mirror+shallow complete, namespaces advanced)**

- `cargo build --release -p grit-rs`: pass
- `./scripts/run-tests.sh t5517-push-mirror.sh`: 13/13
- `./scripts/run-tests.sh t5538-push-shallow.sh`: 8/8
- `./scripts/run-tests.sh t5509-fetch-push-namespaces.sh`: 13/15 (remaining: `transfer.hideRefs` namespace-stripping semantics in cases 6 and 10)

**2026-04-10 (phase 5 / HTTP shallow push + fsck parity)**

- `cargo build --release -p grit-rs`: pass
- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo clippy --fix --allow-dirty -p grit-rs`: pass (reverted unrelated formatting-only edits before commit)
- `cargo test -p grit-lib --lib`: 166 passed
- `./scripts/run-tests.sh t5549-fetch-push-http.sh`: 3/3
- `./scripts/run-tests.sh t5542-push-http-shallow.sh`: 3/3
- `./scripts/run-tests.sh t5545-push-options.sh`: 13/13
- `./scripts/run-tests.sh t5516-fetch-push.sh`: 52/124 (broad integration baseline; not a regression target for this slice)

**2026-04-10 (phase 4 / atomic push parity)**

- `cargo check -p grit-rs`: pass
- `cargo clippy --fix --allow-dirty -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `./scripts/run-tests.sh t5543-atomic-push.sh`: 13/13

**2026-04-10 (t5545 / push options parity incl. submodules)**

- `cargo check -p grit-rs`: pass
- `cargo clippy --fix --allow-dirty -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `./scripts/run-tests.sh t5528-push-default.sh`: 31/32 (`1` expected upstream `test_expect_failure`)
- `./scripts/run-tests.sh t5533-push-cas.sh`: 23/23
- `./scripts/run-tests.sh t5545-push-options.sh`: 13/13

**2026-04-10 (t5533 / push CAS + force-if-includes)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `./scripts/run-tests.sh t5533-push-cas.sh`: 23/23

**2026-04-10 (t5528 / push.default semantics)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `./tests/t5528-push-default.sh -v`: 31/32 pass (`1` expected upstream `test_expect_failure`)
- `./scripts/run-tests.sh t5528-push-default.sh`: 31/32

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
