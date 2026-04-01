# Gust v2 implementation plan

This plan is the **second major version** of Gust: same working method as v1 ([`plan.md`](../plan.md), [`AGENT.md`](../AGENT.md))—study the **upstream C** in [`git/builtin/`](../git/builtin/), the **manpage sources** in [`git/Documentation/`](../git/Documentation/) as `git-<command>.adoc`, and the **tests** under [`git/t/`](../git/t/); **port** chosen scripts into `./tests` (substitute `gust` for `git` per the harness contract) and drive implementation until those tests pass.

**v2 scope:** additional **basic plumbing** (and closely related maintenance commands) beyond the eleven v1 subcommands. Commands in this release:

| Command | Upstream builtin | Documentation |
|--------|------------------|---------------|
| `check-ignore` | [`git/builtin/check-ignore.c`](../git/builtin/check-ignore.c) | `git/Documentation/git-check-ignore.adoc` |
| `count-objects` | [`git/builtin/count-objects.c`](../git/builtin/count-objects.c) | `git/Documentation/git-count-objects.adoc` |
| `diff-index` | [`git/builtin/diff-index.c`](../git/builtin/diff-index.c) | `git/Documentation/git-diff-index.adoc` |
| `for-each-ref` | [`git/builtin/for-each-ref.c`](../git/builtin/for-each-ref.c) | `git/Documentation/git-for-each-ref.adoc` |
| `merge-base` | [`git/builtin/merge-base.c`](../git/builtin/merge-base.c) | `git/Documentation/git-merge-base.adoc` |
| `rev-list` | [`git/builtin/rev-list.c`](../git/builtin/rev-list.c) | `git/Documentation/git-rev-list.adoc` |
| `rev-parse` | [`git/builtin/rev-parse.c`](../git/builtin/rev-parse.c) | `git/Documentation/git-rev-parse.adoc` |
| `show-ref` | [`git/builtin/show-ref.c`](../git/builtin/show-ref.c) | `git/Documentation/git-show-ref.adoc` |
| `symbolic-ref` | [`git/builtin/symbolic-ref.c`](../git/builtin/symbolic-ref.c) | `git/Documentation/git-symbolic-ref.adoc` |
| `verify-pack` | [`git/builtin/verify-pack.c`](../git/builtin/verify-pack.c) | `git/Documentation/git-verify-pack.adoc` |
| `gc` | [`git/builtin/gc.c`](../git/builtin/gc.c) | `git/Documentation/git-gc.adoc` |
| `repack` | [`git/builtin/repack.c`](../git/builtin/repack.c) | `git/Documentation/git-repack.adoc` |

**Checkbox legend:** `[ ]` not started · `[~]` claimed · `[x]` done and tests green.

---

## Phase 0 — v2 infrastructure and CLI

Work that spans multiple commands; finish enough of each before the commands that depend on it.

- [x] **0.1 CLI registration** — Dispatch all twelve v2 subcommands from `gust`; global options (`--git-dir`, `-C`, etc.) as required by ported tests; usage strings aligned with upstream.
- [ ] **0.2 Revision parsing (`rev-parse` core)** — DWIM refs, `^{}`, peel tags, ambiguous object disambiguation behavior, pathspec/`--` boundaries, and plumbing output modes (`--verify`, `--short`, object type/size flags, etc.) as needed by later phases (much of this surfaces in `rev-parse` itself).
- [ ] **0.3 Reachability and walk** — Commit graph traversal, parent ordering, and primitives shared by `merge-base` and `rev-list` (match `revision.c` / libgit walk semantics for the subset you port).
- [ ] **0.4 Ignore rules** — `.gitignore`, `.git/info/exclude`, `core.excludesfile`, optional index integration for `--no-index` vs index-aware paths; pattern syntax and precedence aligned with upstream for `check-ignore`.
- [ ] **0.5 Packfiles** — Read `.pack` + `.idx` (and any options tests need: thin packs, promisor bits, etc. only if selected tests require). Required for `count-objects -v`, `verify-pack`, `repack`, and `gc`.
- [ ] **0.6 Pack writing / maintenance hooks** — Ability to build or rewrite packs and prune loose objects as `repack` and `gc` demand (may pull in behavior from `pack-objects` / `prune` internally even if those remain non-user-facing).

---

## Phase 1 — `rev-parse`

**Upstream:** [`git/builtin/rev-parse.c`](../git/builtin/rev-parse.c). **Docs:** `git/Documentation/git-rev-parse.adoc`.

**Primary tests (representative; expand as needed):**

- [`git/t/t1500-rev-parse.sh`](../git/t/t1500-rev-parse.sh)
- [`git/t/t1502-rev-parse-parseopt.sh`](../git/t/t1502-rev-parse-parseopt.sh) — include only if v2 exposes `parseopt` / `--git-common-dir` family consistently with chosen scripts
- [`git/t/t1503-rev-parse-verify.sh`](../git/t/t1503-rev-parse-verify.sh)
- [`git/t/t1505-rev-parse-last.sh`](../git/t/t1505-rev-parse-last.sh)
- [`git/t/t1506-rev-parse-diagnosis.sh`](../git/t/t1506-rev-parse-diagnosis.sh)
- [`git/t/t1507-rev-parse-upstream.sh`](../git/t/t1507-rev-parse-upstream.sh)
- [`git/t/t1511-rev-parse-caret.sh`](../git/t/t1511-rev-parse-caret.sh)
- [`git/t/t1512-rev-parse-disambiguation.sh`](../git/t/t1512-rev-parse-disambiguation.sh)
- [`git/t/t1513-rev-parse-prefix.sh`](../git/t/t1513-rev-parse-prefix.sh)
- [`git/t/t1514-rev-parse-push.sh`](../git/t/t1514-rev-parse-push.sh)
- [`git/t/t1515-rev-parse-outside-repo.sh`](../git/t/t1515-rev-parse-outside-repo.sh)
- [`git/t/t6101-rev-parse-parents.sh`](../git/t/t6101-rev-parse-parents.sh)

- [ ] **1.1** Repository vs non-repository modes; `--is-inside-work-tree`, `--show-toplevel`, `--git-dir`, `--show-prefix`, and related discovery flags used in tests.
- [ ] **1.2** Parse revisions and object names; `--verify`, short/long hashes, `^{}` peeling, ref@upstream forms as in scope.
- [ ] **1.3** Quoted path / magic pathspec handling only if ported scripts require it.
- [ ] **1.4** Port selected `t150*.sh` / `t6101-rev-parse-parents.sh` scripts; defer parseopt-heavy behavior unless explicitly in scope.

---

## Phase 2 — `symbolic-ref` and `show-ref`

**Upstream:** [`git/builtin/symbolic-ref.c`](../git/builtin/symbolic-ref.c), [`git/builtin/show-ref.c`](../git/builtin/show-ref.c). **Docs:** `git/Documentation/git-symbolic-ref.adoc`, `git/Documentation/git-show-ref.adoc`.

**Primary tests:**

- [`git/t/t1401-symbolic-ref.sh`](../git/t/t1401-symbolic-ref.sh)
- [`git/t/t1403-show-ref.sh`](../git/t/t1403-show-ref.sh)
- [`git/t/t1422-show-ref-exists.sh`](../git/t/t1422-show-ref-exists.sh)

- [ ] **2.1** `symbolic-ref`: read/create/delete symbolic refs; validate ref targets; error messages matching upstream.
- [ ] **2.2** `show-ref`: list refs with patterns, `--heads`, `--tags`, `--verify`, `-d`/`--dereference`, `-s`/`--hash`, exit codes for missing refs.
- [ ] **2.3** Port and pass the `t1401` / `t1403` / `t1422` scripts (and shared helpers those files need).

---

## Phase 3 — `check-ignore`

**Upstream:** [`git/builtin/check-ignore.c`](../git/builtin/check-ignore.c). **Docs:** `git/Documentation/git-check-ignore.adoc`. **Primary tests:** [`git/t/t0008-ignores.sh`](../git/t/t0008-ignores.sh) (description explicitly targets `check-ignore`; large file—implement incrementally).

- [ ] **3.1** Path arguments and `-z`, `-n` (dry run), `-v` / `-vv`, `--stdin`, `--no-index`, `--non-matching` as used in `t0008`.
- [ ] **3.2** Correct interaction with working tree, index, and nested `.gitignore` / exclude files; directory vs file semantics; trailing-slash rules.
- [ ] **3.3** Port relevant sections of `t0008-ignores.sh` (or the whole script if feasible); skip attr-only magic pathspec cases unless v2 expands pathspec attribute support.

---

## Phase 4 — `merge-base`

**Upstream:** [`git/builtin/merge-base.c`](../git/builtin/merge-base.c). **Docs:** `git/Documentation/git-merge-base.adoc`. **Primary tests:** [`git/t/t6010-merge-base.sh`](../git/t/t6010-merge-base.sh); related: [`git/t/t4068-diff-symmetric-merge-base.sh`](../git/t/t4068-diff-symmetric-merge-base.sh) if `diff` plumbing pulls it in.

- [ ] **4.1** Default merge-base selection; `--all`, `--octopus`, `--independent`, `--is-ancestor`.
- [ ] **4.2** Corner cases: disjoint histories, root commits, same commit repeated.
- [ ] **4.3** Port and pass `t6010-merge-base.sh` (and any merge-base-dependent chunks of other chosen scripts).

---

## Phase 5 — `rev-list`

**Upstream:** [`git/builtin/rev-list.c`](../git/builtin/rev-list.c). **Docs:** `git/Documentation/git-rev-list.adoc`.

**Tests:** Upstream coverage is broad ([`git/t/t6000-rev-list-misc.sh`](../git/t/t6000-rev-list-misc.sh) through [`git/t/t6022-rev-list-missing.sh`](../git/t/t6022-rev-list-missing.sh), format/graph/bitmap variants, etc.). **Strategy:** start with a minimal set (ordering, `--max-count`, `--skip`, basic `--parents`, `--objects`, `--filter` only if ported), then add scripts from `t600*.sh` until the agreed v2 bar is met.

Representative files to prioritize:

- [`git/t/t6000-rev-list-misc.sh`](../git/t/t6000-rev-list-misc.sh)
- [`git/t/t6003-rev-list-topo-order.sh`](../git/t/t6003-rev-list-topo-order.sh)
- [`git/t/t6005-rev-list-count.sh`](../git/t/t6005-rev-list-count.sh)
- [`git/t/t6006-rev-list-format.sh`](../git/t/t6006-rev-list-format.sh)
- [`git/t/t6014-rev-list-all.sh`](../git/t/t6014-rev-list-all.sh)
- [`git/t/t6017-rev-list-stdin.sh`](../git/t/t6017-rev-list-stdin.sh)

- [ ] **5.1** Commit walking: `--first-parent`, `--ancestry-path`, `--simplify-by-decoration`, simplification flags as required by chosen tests.
- [ ] **5.2** Ordering: topo, date, reverse; `--objects` / `--object-names` / `--filter-print-omitted` only if in scope.
- [ ] **5.3** Output formatting: `--format`, hash-only modes, `--quiet` / exit code conventions.
- [ ] **5.4** Bitmap or lazy promisor behavior — **defer** unless a ported test requires it.
- [ ] **5.5** Port agreed `t600*.sh` subset; document explicitly which rev-list features remain out of scope for v2 if not all upstream tests are targeted.

---

## Phase 6 — `diff-index`

**Upstream:** [`git/builtin/diff-index.c`](../git/builtin/diff-index.c). **Docs:** `git/Documentation/git-diff-index.adoc`.

**Primary tests (representative; many scripts call `git diff-index` in passing):**

- [`git/t/t4013-diff-various.sh`](../git/t/t4013-diff-various.sh) (includes `diff-index -m` behavior)
- [`git/t/t4017-diff-retval.sh`](../git/t/t4017-diff-retval.sh)
- [`git/t/t4044-diff-index-unique-abbrev.sh`](../git/t/t4044-diff-index-unique-abbrev.sh)

- [ ] **6.1** Compare index vs tree / HEAD: `--cached`, `--merge`, `-m`, `--exit-code`, `--quiet`, `--raw`, `-p` (patch) paths used in tests.
- [ ] **6.2** Diff options shared with `git diff` (rename, indent heuristic, etc.) only as required by ported scripts.
- [ ] **6.3** Pathspec handling and stat/cache interaction consistent with v1 index behavior.
- [ ] **6.4** Port selected diff-index-heavy scripts; avoid pulling the entire `t4000` diff suite unless v2 explicitly widens scope.

---

## Phase 7 — `for-each-ref`

**Upstream:** [`git/builtin/for-each-ref.c`](../git/builtin/for-each-ref.c). **Docs:** `git/Documentation/git-for-each-ref.adoc`. **Shared:** [`git/t/for-each-ref-tests.sh`](../git/t/for-each-ref-tests.sh) if sourced by other tests.

**Primary tests:**

- [`git/t/t6300-for-each-ref.sh`](../git/t/t6300-for-each-ref.sh)
- [`git/t/t6301-for-each-ref-errors.sh`](../git/t/t6301-for-each-ref-errors.sh)
- [`git/t/t6302-for-each-ref-filter.sh`](../git/t/t6302-for-each-ref-filter.sh)

- [ ] **7.1** Ref sorting (`--sort`), patterns, `--count`, `--format` atoms matching upstream for covered tests.
- [ ] **7.2** Filter/query language (`--contains`, `--merged`, `--no-merged`, `--points-at`, etc.) as required by `t630*`.
- [ ] **7.3** Error handling and stdin/parse edge cases from `t6301`.
- [ ] **7.4** Port `t6300` / `t6301` / `t6302` (or an agreed subset).

---

## Phase 8 — `count-objects` and `verify-pack`

**Upstream:** [`git/builtin/count-objects.c`](../git/builtin/count-objects.c), [`git/builtin/verify-pack.c`](../git/builtin/verify-pack.c). **Docs:** `git/Documentation/git-count-objects.adoc`, `git/Documentation/git-verify-pack.adoc`.

**Primary tests (pick a coherent pack-focused set):**

- [`git/t/t5301-sliding-window.sh`](../git/t/t5301-sliding-window.sh) — contains dedicated `verify-pack -v` cases
- [`git/t/t5304-prune.sh`](../git/t/t5304-prune.sh) — uses `count-objects` for loose object counts
- [`git/t/t5613-info-alternate.sh`](../git/t/t5613-info-alternate.sh) — `count-objects -v` with alternates

- [ ] **8.1** `count-objects`: default summary; `-v` / `--verbose` breakdown (packs, loose, duplicates, alternates) per upstream output format.
- [ ] **8.2** `verify-pack`: `-v` statistics, object enumeration, corruption detection and exit codes; optional object format flag if tests use it.
- [ ] **8.3** Port selected scripts; omit multi-pack-index / promisor-only behavior unless those tests are in the v2 list.

---

## Phase 9 — `repack`

**Upstream:** [`git/builtin/repack.c`](../git/builtin/repack.c). **Docs:** `git/Documentation/git-repack.adoc`.

**Primary tests:**

- [`git/t/t7700-repack.sh`](../git/t/t7700-repack.sh)
- [`git/t/t7701-repack-unpack-unreachable.sh`](../git/t/t7701-repack-unpack-unreachable.sh)
- [`git/t/t7702-repack-cyclic-alternate.sh`](../git/t/t7702-repack-cyclic-alternate.sh)
- [`git/t/t7703-repack-geometric.sh`](../git/t/t7703-repack-geometric.sh)
- [`git/t/t7704-repack-cruft.sh`](../git/t/t7704-repack-cruft.sh)

- [ ] **9.1** Basic repack into single or multiple packs; `-a`, `-A`, `-d`, `-l`, `-f`, `-F`, `--window`, `--depth` as tests require.
- [ ] **9.2** Cruft packs, geometric factor, keep-unreachable behavior — gated on which `t770*` scripts are ported.
- [ ] **9.3** Interaction with alternates and pack reuse from `t7702`.
- [ ] **9.4** Port agreed `t770*.sh` subset; treat full geometric/cruft coverage as optional if scope is constrained.

---

## Phase 10 — `gc`

**Upstream:** [`git/builtin/gc.c`](../git/builtin/gc.c). **Docs:** `git/Documentation/git-gc.adoc`. **Primary tests:** [`git/t/t6500-gc.sh`](../git/t/t6500-gc.sh).

- [ ] **10.1** Default `gc`: pack loose objects, prune, run `repack` / `prune` pipeline per config knobs used in `t6500`.
- [ ] **10.2** Honor `gc.*` configuration (`auto`, `packrefs`, `worktrees`, etc.) as required by ported tests.
- [ ] **10.3** Safe behavior with hooks, reflog expiry, and `--prune=` only if in scope.
- [ ] **10.4** Port and pass `t6500-gc.sh` (or document deferrals for auto-gc / daemon scenarios).

---

## Explicit deferrals and notes

- **Submodule-heavy tests:** Many `rev-list`, `diff-index`, and `gc` tests assume submodule plumbing; defer or stub unless v2 adds submodule support.
- **Reftable:** If upstream tests assume `reftable` ref backend, scope v2 to **files** backend unless the project expands `AGENT.md`.
- **Partial / promisor clones:** Only implement `verify-pack` / `count-objects` / `gc` behaviors for promisor packs if selected tests require them.
- **Performance tests:** `git/t/perf/*` is not required for correctness unless the project explicitly tracks perf parity.

---

## Completion criteria

v2 is “done” when:

1. All twelve commands are exposed as `gust <command>` with behavior matching upstream for the **ported** test set.
2. Manpage parity (or a tracked checklist similar to [`docs/manpage-parity.md`](../docs/manpage-parity.md)) is updated for v2 commands.
3. `cargo test --workspace` and `./tests/harness/run.sh` (with v2 entries in `tests/harness/selected-tests.txt`) succeed for the agreed script list.
