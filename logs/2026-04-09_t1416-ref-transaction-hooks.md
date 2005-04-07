# t1416-ref-transaction-hooks

## Failure

Test 10 expected an extra `reference-transaction` `aborted` phase (with stdin `0 0 refs/heads/symrefd`) between `preparing` and `prepared` on the files backend, matching Git’s nested packed-refs transaction abort.

## Root cause

1. `grit pack-refs` used `list_refs`, which resolves symbolic refs to OIDs and wrote bogus lines into `packed-refs`. Git’s packed format cannot store symrefs; those refs must stay loose only.
2. That left `symrefd` appearing “in packed-refs”, so our hook logic (correctly mirroring `is_packed_transaction_needed`) skipped the synthetic `aborted` call.

## Fix

- `pack_refs`: walk loose files under `refs/`, pack only `Ref::Direct`, remove packed entries for loose symrefs, delete `packed-refs` when empty.
- `grit-lib`: `refs::packed_refs_entry_exists` for hook/packed checks.
- `ref_transaction_hooks` module: between `preparing` and `prepared`, on non-reftable repos, if no deleted ref in the batch has a packed line, run one `aborted` hook with `0 0 <ref>` per deletion (single invocation, multiple stdin lines).
- `symbolic-ref --delete`: use `delete_ref` so packed-refs is cleaned like Git.
- `update_ref` / `symbolic_ref`: shared `HookUpdate` with `deletes_ref` so verify ops don’t trigger the packed preview.

## Verify

`./scripts/run-tests.sh t1416-ref-transaction-hooks.sh` → 10/10.
