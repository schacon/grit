---
title: Parallelism
summary: Thread policy, Git config keys, and ordered parallel maps in grit-lib.
---

Grit centralizes worker counts and batch thresholds in [`grit_lib::parallel`](https://docs.rs/grit-lib/latest/grit_lib/parallel/index.html) so embedders and the CLI share one policy.

## Thread policy

[`ThreadPolicy`](https://docs.rs/grit-lib/latest/grit_lib/parallel/struct.ThreadPolicy.html) holds optional overrides:

- **`threads_override`** — explicit worker count (`0` = auto / all logical CPUs, `1` = serial). The `grit` CLI sets this from global [`--threads`](../global-options.md).
- **`min_items_override`** — minimum input length before spawning a pool. Setting it to **`0`** forces the parallel path on tiny inputs (integration tests use this).

Call [`ThreadPolicy::resolve`](https://docs.rs/grit-lib/latest/grit_lib/parallel/struct.ThreadPolicy.html#method.resolve) with a [`ParallelOp`](https://docs.rs/grit-lib/latest/grit_lib/parallel/enum.ParallelOp.html) and a merged [`ConfigSet`](https://docs.rs/grit-lib/latest/grit_lib/config/struct.ConfigSet.html) to obtain [`ParallelOptions`](https://docs.rs/grit-lib/latest/grit_lib/parallel/struct.ParallelOptions.html) (threads + batch thresholds).

| `ParallelOp` | Config keys |
| --- | --- |
| `IndexRefresh` | `core.preloadIndex`, `index.threads` |
| `IndexLoad` | `core.preloadIndex`, `index.threads` |
| `Checkout` | `checkout.workers`, `checkout.thresholdForParallelism` (default threshold `100`) |
| `IndexPack`, `PackWrite` | `pack.threads` |
| `DiffRename` | `index.threads` with `core.preloadIndex` |

### Git value semantics for `index.threads`

Matches Git's `repo_config_get_index_threads`:

- unset — auto (all logical CPUs for hash-style refresh paths)
- `true` or `0` — auto
- `false` or `1` — serial (one worker)
- integer `n > 1` — up to `n` workers (clamped to available CPUs)
- other integers (for example `-3`) — preserved as serial (Git only parallelizes when `nr_threads > 1` after treating `0` as auto)

When `core.preloadIndex` is false, index refresh/load operations stay serial regardless of `index.threads`.

### `pack.threads` (index-pack / pack-objects)

Unset or `0` selects auto (all logical CPUs). Positive integers cap worker count. Git rejects booleans, non-numeric strings, and negative values with a config error; use [`ThreadPolicy::try_resolve`](https://docs.rs/grit-lib/latest/grit_lib/parallel/struct.ThreadPolicy.html#method.try_resolve) for [`ParallelOp::IndexPack`](https://docs.rs/grit-lib/latest/grit_lib/parallel/enum.ParallelOp.html) to surface those errors. Fetch and [`clone`](https://docs.rs/grit-lib/latest/grit_lib/clone/fn.clone.html) pass [`CloneOptions::thread_policy`](https://docs.rs/grit-lib/latest/grit_lib/clone/struct.CloneOptions.html) (and the repository [`Environment`](https://docs.rs/grit-lib/latest/grit_lib/environment/struct.Environment.html)) into received-pack indexing so global `--threads` overrides `pack.threads` and command-scope `GIT_CONFIG_*` / `-c pack.threads=N` still applies when no override is set.

Stat preload during status/diff keeps a separate four-thread cap until a later scaling pass; it still honors `core.preloadIndex` and the global thread override.

Pack writing (`pack-objects` serialization) uses the same `pack.threads` policy for parallel delta encoding and zlib compression; the on-wire byte stream matches the single-threaded writer (OFS vs REF headers are still emitted in plan order).

## Ordered parallel maps

[`try_par_map`](https://docs.rs/grit-lib/latest/grit_lib/parallel/fn.try_par_map.html) and [`par_map`](https://docs.rs/grit-lib/latest/grit_lib/parallel/fn.par_map.html) run a closure over a slice with a scoped thread pool. **Results are always returned in input order**, even when workers finish out of order.

[`try_par_map_force`](https://docs.rs/grit-lib/latest/grit_lib/parallel/fn.try_par_map_force.html) skips batch thresholds (used for index-pack). Hash-specific helpers in [`grit_lib::hash`](https://docs.rs/grit-lib/latest/grit_lib/hash/index.html) remain thin wrappers over these APIs.

There is **no global thread pool** and **no environment reads** inside `grit_lib::parallel`; pass policy explicitly (via [`RepositoryOptions::thread_policy`](https://docs.rs/grit-lib/latest/grit_lib/environment/struct.RepositoryOptions.html) or a local [`ThreadPolicy`](https://docs.rs/grit-lib/latest/grit_lib/parallel/struct.ThreadPolicy.html)).

## Tests

- Policy unit tests live in `grit-lib/src/parallel/policy.rs`.
- Public API coverage: `grit-lib/tests/parallel_api.rs`.
- Hash wrappers: `grit-lib/tests/hash_parallel_api.rs` (unchanged behavior).
- `grit_test_support::forced_parallel_policy` sets `threads = 4` and `min_items = 0` for deterministic parallel paths in integration tests (see the `grit-test-support` crate in the workspace).
