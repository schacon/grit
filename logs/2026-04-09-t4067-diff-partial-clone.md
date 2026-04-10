# t4067-diff-partial-clone

- Fixed `promisor_lazy_fetch_allowed_for_client_process` to match `GIT_NO_LAZY_FETCH` semantics so `clone --filter=blob:limit=0` succeeds without extra env.
- Promisor lazy fetch: batch `want` list, label packet trace as `fetch`, fall back to copying from local promisor ODB when thin-pack indexing fails under `GIT_TRACE_PACKET`.
- `diff`: parse bare-repo revs without treating `HEAD` as a path; compute `rename_threshold` after trailing-flag scan so `-M` works; merge prefetch for rename/break-rewrites/output; honor `--no-renames`.
- `checkout`: retry `odb.read` after `try_lazy_fetch_promisor_object` on missing blobs.
- `show`: promisor prefetch aligned with diff helpers.
- Refreshed harness row + dashboards via `./scripts/run-tests.sh t4067-diff-partial-clone.sh`.
