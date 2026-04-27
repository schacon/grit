# Remote Auth Credential Model

## Scope

Claimed Phase 1 in `AUTH_TASKS.md`: replace the flat `BTreeMap` credential handling in `grit credential` with an ordered credential record that can preserve Git credential protocol fields.

## Changes

- Added an ordered `Credential` model in `grit/src/commands/credential.rs`.
- Preserved repeated `key[]` fields such as `capability[]`, `wwwauth[]`, and `state[]`.
- Implemented array reset semantics for empty `key[]=` entries.
- Kept scalar fields ordered in Git-compatible output order for helper interactions.
- Normalized `url=` into protocol, host, path, username, and password without losing the original context.
- Removed HTTP(S) path before helper invocation unless `credential.useHttpPath` is enabled.
- Added helper response merging that can represent pre-encoded `authtype` + `credential` responses.
- Added expiry filtering for `password_expiry_utc`.
- Added helper-chain stopping once a complete username/password or pre-encoded credential is available.
- Added `GIT_ASKPASS`, `core.askPass`, and `SSH_ASKPASS` lookup for missing credential prompts.

## Validation

- `cargo check -p grit-rs`: passed.
- `cargo build --release -p grit-rs`: passed.
- `cargo test -p grit-lib --lib`: 197 passed.
- `./scripts/run-tests.sh t0300-credentials.sh`: skipped because `t0300-credentials` is currently `in_scope=skip` in `data/test-files.csv`.
- Manual smoke checks with `target/release/grit credential fill` verified:
  - Basic username/password helper output.
  - Capability-aware `authtype` + `credential` helper output.
  - Capability filtering when the caller did not advertise `authtype` support.

## Remaining Work

- Implement full `credential.protectProtocol` validation.
- Implement prompt sanitization for control characters.
- Continue Phase 2 helper semantics, including `grit credential capability`, exact `quit` stderr wording, and URL-scoped username behavior.
