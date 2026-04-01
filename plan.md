# Gust v1 implementation plan

This plan implements the **v1 plumbing-only** scope from [`AGENT.md`](AGENT.md): eleven subcommands exposed as `gust <command>`, matching Git behavior well enough to pass the **ported** tests from [`git/t/`](git/t/) (copied into `./tests` with `gust` substituted for `git` per the build contract).

**Canonical upstream code:** C implementation under [`git/builtin/`](git/builtin/) (e.g. `init-db.c` for `git init`, `hash-object.c`, `update-index.c`, …). **Reference docs:** [`git/Documentation/`](git/Documentation/) as `git-<command>.adoc` (and related includes).

**Checkbox legend (for autonomous loops):** `[ ]` not started · `[~]` claimed · `[x]` done and tests green.

---

## Phase 0 — Workspace and shared infrastructure

These blocks underpin every command; order matters less than completing them before higher-level features.

- [x] **0.1 Cargo workspace** — Binary crate `gust` (or workspace root + `gust` bin), library crate for core types, `anyhow` / `thiserror` split per AGENT.md, lints denying `unwrap`/`expect` in production paths.
- [x] **0.2 CLI** — Subcommand dispatch for all eleven v1 commands; global options Git honors where required (`--git-dir`, `-C`, etc.) as needed by tests; stable `gust --help` / per-command help aligned with upstream usage strings.
- [x] **0.3 Repository discovery** — Resolve `GIT_DIR`, work tree, bare vs non-bare, `core.bare`, `.git` file (gitfile) indirection; fail with same classes of errors as Git on broken setups (see [`git/t/t0009-git-dir-validation.sh`](git/t/t0009-git-dir-validation.sh), [`git/t/t0002-gitfile.sh`](git/t/t0002-gitfile.sh) when ported).
- [x] **0.4 Object ID and hashing** — SHA-1 object names (hex, loose object directory layout `objects/ab/cdef...`); no `gix` / `git2`; document public OID type.
- [x] **0.5 Loose object store** — Read/write zlib-compressed loose objects; headers `blob` / `tree` / `commit` / `tag` with size; corruption errors consistent enough for tests.
- [x] **0.6 Test harness** — Process to **copy** selected scripts from `git/t/` into `./tests`, replace `git` with `gust` only where appropriate, and run them; do not author unrelated test suites (per AGENT.md).

---

## Phase 1 — `init`

**Upstream:** [`git/builtin/init-db.c`](git/builtin/init-db.c). **Docs:** `git/Documentation/git-init.adoc`. **Primary tests:** [`git/t/t0001-init.sh`](git/t/t0001-init.sh); also [`git/t/t1301-shared-repo.sh`](git/t/t1301-shared-repo.sh) for `--shared`.

- [x] **1.1** Create `objects`, `refs`, `refs/heads`, template hooks/info (as upstream), default `HEAD` target.
- [x] **1.2** Flags: `--bare`, `--quiet`, `--template=`, `--separate-git-dir`, `--object-format=` (at least `sha1` for v1), `--ref-format=` if required by chosen tests, `--initial-branch` / `-b`, `--shared` with permission modes.
- [x] **1.3** Init inside existing vs new directory; respect path argument.
- [x] **1.4** Port and pass `t0001-init.sh` (and any shared-repo cases in scope).

---

## Phase 2 — `hash-object`

**Upstream:** [`git/builtin/hash-object.c`](git/builtin/hash-object.c). **Docs:** `git/Documentation/git-hash-object.adoc`. **Primary tests:** [`git/t/t1007-hash-object.sh`](git/t/t1007-hash-object.sh).

- [x] **2.1** Compute hash from stdin or file path (`-w` write to object store, `-t` type, `--no-filters` / filter path if tests require).
- [x] **2.2** Pathspec / `--path` behavior matching upstream for covered tests.
- [x] **2.3** Literal (`--literally`) and stdin consumption modes used in tests.
- [x] **2.4** Port and pass `t1007-hash-object.sh`.

---

## Phase 3 — `cat-file`

**Upstream:** [`git/builtin/cat-file.c`](git/builtin/cat-file.c). **Docs:** `git/Documentation/git-cat-file.adoc`. **Primary tests:** [`git/t/t1006-cat-file.sh`](git/t/t1006-cat-file.sh). **Optional / feature-gated in upstream:** [`git/t/t8007-cat-file-textconv.sh`](git/t/t8007-cat-file-textconv.sh), [`git/t/t8010-cat-file-filters.sh`](git/t/t8010-cat-file-filters.sh) — include only if v1 explicitly expands to filters/textconv.

- [x] **3.1** Modes: `-t`, `-s`, `-p`, existence checks; dereference tags where specified (`-p` on tags).
- [x] **3.2** Batch modes: `--batch`, `--batch-check`, `--batch-command` as required by `t1006-cat-file.sh`.
- [x] **3.3** Object parsing for `tree`/`commit`/`blob` display (pretty-print trees/commits like Git).
- [x] **3.4** Port and pass `t1006-cat-file.sh` (and decide on filter/mailmap tests for v1).

---

## Phase 4 — Index: `update-index` and `ls-files`

**Upstream:** [`git/builtin/update-index.c`](git/builtin/update-index.c), [`git/builtin/ls-files.c`](git/builtin/ls-files.c). **Docs:** `git/Documentation/git-update-index.adoc`, `git/Documentation/git-ls-files.adoc`.

**update-index tests (representative):**

- [`git/t/t2107-update-index-basic.sh`](git/t/t2107-update-index-basic.sh)
- [`git/t/t2103-update-index-ignore-missing.sh`](git/t/t2103-update-index-ignore-missing.sh)
- [`git/t/t2102-update-index-symlinks.sh`](git/t/t2102-update-index-symlinks.sh)
- [`git/t/t2101-update-index-reupdate.sh`](git/t/t2101-update-index-reupdate.sh)
- [`git/t/t2105-update-index-gitfile.sh`](git/t/t2105-update-index-gitfile.sh)
- [`git/t/t2106-update-index-assume-unchanged.sh`](git/t/t2106-update-index-assume-unchanged.sh)
- [`git/t/t2108-update-index-refresh-racy.sh`](git/t/t2108-update-index-refresh-racy.sh) (timestamp fidelity)
- [`git/t/t2100-update-cache-badpath.sh`](git/t/t2100-update-cache-badpath.sh)
- [`git/t/t0055-beyond-symlinks.sh`](git/t/t0055-beyond-symlinks.sh) (with `add`-like paths — only if tests call `update-index`)
- [`git/t/t0004-unwritable.sh`](git/t/t0004-unwritable.sh) (permissions)

**ls-files tests (representative):**

- [`git/t/t3004-ls-files-basic.sh`](git/t/t3004-ls-files-basic.sh)
- [`git/t/t3000-ls-files-others.sh`](git/t/t3000-ls-files-others.sh)
- [`git/t/t3001-ls-files-others-exclude.sh`](git/t/t3001-ls-files-others-exclude.sh)
- [`git/t/t3002-ls-files-dashpath.sh`](git/t/t3002-ls-files-dashpath.sh)
- [`git/t/t3003-ls-files-exclude.sh`](git/t/t3003-ls-files-exclude.sh)
- [`git/t/t3005-ls-files-relative.sh`](git/t/t3005-ls-files-relative.sh)
- [`git/t/t3006-ls-files-long.sh`](git/t/t3006-ls-files-long.sh)
- [`git/t/t3007-ls-files-recurse-submodules.sh`](git/t/t3007-ls-files-recurse-submodules.sh) — may defer if submodules out of v1
- [`git/t/t3008-ls-files-lazy-init-name-hash.sh`](git/t/t3008-ls-files-lazy-init-name-hash.sh)
- [`git/t/t3009-ls-files-others-nonsubmodule.sh`](git/t/t3009-ls-files-others-nonsubmodule.sh)
- [`git/t/t3010-ls-files-killed-modified.sh`](git/t/t3010-ls-files-killed-modified.sh)
- [`git/t/t3012-ls-files-dedup.sh`](git/t/t3012-ls-files-dedup.sh)
- [`git/t/t3013-ls-files-format.sh`](git/t/t3013-ls-files-format.sh)
- [`git/t/t3020-ls-files-error-unmatch.sh`](git/t/t3020-ls-files-error-unmatch.sh)
- [`git/t/t3060-ls-files-with-tree.sh`](git/t/t3060-ls-files-with-tree.sh)

**Shared index work:**

- [x] **4.1** Parse and write Git index format (endianness, fixed extensions skipped or implemented as tests demand — e.g. `cache-tree`, `split-index`, `sparse` as needed).
- [x] **4.2** `update-index`: add/remove/cacheinfo, `--add`, `--remove`, `--force-remove`, `--info-only`, `--index-info`, racy-git / stat refresh paths, `--really-refresh`, `--again`, flags `assume-unchanged`, `skip-worktree` if in chosen tests.
- [x] **4.3** `ls-files`: default listing, `-s`/stages, `-o`/`--others`, `-i`/`--ignored`, `-m`/`--modified`, `-d`/`--deleted`, `-k`/`--killed`, `-u`/`--unmerged`, `-z`, `--exclude`, `--exclude-from`, pathspecs, `--format`, `--deduplicate`, `--error-unmatch`, `--with-tree`.
- [x] **4.4** Port chosen update-index and ls-files scripts; mark submodule-heavy tests as deferred or implement minimal stub behavior.

---

## Phase 5 — `write-tree`

**Upstream:** [`git/builtin/write-tree.c`](git/builtin/write-tree.c). **Docs:** `git/Documentation/git-write-tree.adoc`. **Tests:** [`git/t/t0000-basic.sh`](git/t/t0000-basic.sh) (multiple `write-tree` cases); [`git/t/t0090-cache-tree.sh`](git/t/t0090-cache-tree.sh) if cache-tree extension is implemented; [`git/t/t1020-subdirectory.sh`](git/t/t1020-subdirectory.sh).

- [x] **5.1** Build tree object from index (sorted entries, correct modes for blob/executable/symlink).
- [x] **5.2** `--prefix` partial tree write; `--missing-ok` behavior.
- [x] **5.3** Update or validate `cache-tree` extension when tests assert it (via `test-tool` in upstream — may need equivalent validation or ported expectations).
- [x] **5.4** Port and pass relevant `write-tree` sections from `t0000-basic.sh` and any dedicated scripts.

---

## Phase 6 — `ls-tree`

**Upstream:** [`git/builtin/ls-tree.c`](git/builtin/ls-tree.c). **Docs:** `git/Documentation/git-ls-tree.adoc`. **Tests:** [`git/t/t3100-ls-tree-restrict.sh`](git/t/t3100-ls-tree-restrict.sh), [`git/t/t3101-ls-tree-dirname.sh`](git/t/t3101-ls-tree-dirname.sh), [`git/t/t3102-ls-tree-wildcards.sh`](git/t/t3102-ls-tree-wildcards.sh), [`git/t/t3104-ls-tree-format.sh`](git/t/t3104-ls-tree-format.sh), [`git/t/t3105-ls-tree-output.sh`](git/t/t3105-ls-tree-output.sh), [`git/t/t3902-quoted.sh`](git/t/t3902-quoted.sh) (quoted output).

- [x] **6.1** Walk tree objects; `-d`, `-r`, `-t`, `--long`, name-only / name-status variants per man page.
- [x] **6.2** Pathspec / glob restriction semantics matching upstream.
- [x] **6.3** `--format` field placeholders as required by tests.
- [x] **6.4** Port and pass selected `t310*` and quoting tests.

---

## Phase 7 — `read-tree`

**Upstream:** [`git/builtin/read-tree.c`](git/builtin/read-tree.c). **Docs:** `git/Documentation/git-read-tree.adoc`. **Large test surface** — implement incrementally:

| Script                                                                                 | Focus                         |
| -------------------------------------------------------------------------------------- | ----------------------------- |
| [`git/t/t1009-read-tree-new-index.sh`](git/t/t1009-read-tree-new-index.sh)             | Fresh index                   |
| [`git/t/t1008-read-tree-overlay.sh`](git/t/t1008-read-tree-overlay.sh)                 | Multi-tree overlay            |
| [`git/t/t1005-read-tree-reset.sh`](git/t/t1005-read-tree-reset.sh)                     | `-u --reset`                  |
| [`git/t/t1001-read-tree-m-2way.sh`](git/t/t1001-read-tree-m-2way.sh)                   | 2-way merge `-m`              |
| [`git/t/t1002-read-tree-m-u-2way.sh`](git/t/t1002-read-tree-m-u-2way.sh)               | 2-way with `-u`               |
| [`git/t/t1004-read-tree-m-u-wf.sh`](git/t/t1004-read-tree-m-u-wf.sh)                   | Working tree file checks      |
| [`git/t/t1000-read-tree-m-3way.sh`](git/t/t1000-read-tree-m-3way.sh)                   | 3-way merge                   |
| [`git/t/t1012-read-tree-df.sh`](git/t/t1012-read-tree-df.sh)                           | D/F conflicts                 |
| [`git/t/t1014-read-tree-confusing.sh`](git/t/t1014-read-tree-confusing.sh)             | Reject bad paths              |
| [`git/t/t1003-read-tree-prefix.sh`](git/t/t1003-read-tree-prefix.sh)                   | `--prefix`                    |
| [`git/t/t1013-read-tree-submodule.sh`](git/t/t1013-read-tree-submodule.sh)             | Submodules (defer or minimal) |
| [`git/t/t1022-read-tree-partial-clone.sh`](git/t/t1022-read-tree-partial-clone.sh)     | Promisor / partial clone      |
| [`git/t/t1011-read-tree-sparse-checkout.sh`](git/t/t1011-read-tree-sparse-checkout.sh) | Sparse (likely defer)         |

- [x] **7.1** Single-tree read into empty or existing index (no merge).
- [ ] **7.2** `-m` two-tree and three-tree merge rules (trivial, non-trivial, conflicts).
- [ ] **7.3** `-u` / `--reset` integration with working tree (where tests require).
- [ ] **7.4** `--prefix`, aggressive / trivial merge driver flags only if tests need them.
- [ ] **7.5** Port merge tests in dependency order (new index → overlay → 2-way → 3-way → edge cases).

---

## Phase 8 — `checkout-index`

**Upstream:** [`git/builtin/checkout-index.c`](git/builtin/checkout-index.c). **Docs:** `git/Documentation/git-checkout-index.adoc`. **Tests:** [`git/t/t2006-checkout-index-basic.sh`](git/t/t2006-checkout-index-basic.sh), [`git/t/t2002-checkout-cache-u.sh`](git/t/t2002-checkout-cache-u.sh), [`git/t/t2003-checkout-cache-mkdir.sh`](git/t/t2003-checkout-cache-mkdir.sh), [`git/t/t2004-checkout-cache-temp.sh`](git/t/t2004-checkout-cache-temp.sh), [`git/t/t2005-checkout-index-symlinks.sh`](git/t/t2005-checkout-index-symlinks.sh); parallel/attribute tests ([`git/t/t2080-parallel-checkout-basics.sh`](git/t/t2080-parallel-checkout-basics.sh) etc.) — **defer** unless v1 includes parallel checkout.

- [x] **8.1** Check out all or listed index entries to working tree; create missing directories (`--mkdir`).
- [x] **8.2** Flags: `-a`, `--force`, `-u` (stat refresh), `-q`, `-n` (dry run), `-z` with stdin path list, `--stdin`, `--prefix`, `--stage=all`.
- [x] **8.3** `--temp` / `--tmpdir` for scripted callers.
- [x] **8.4** Symlink vs no-symlink filesystem behavior from `t2005`.
- [x] **8.5** Port and pass basic checkout-index scripts; skip `checkout` (porcelain) tests.

---

## Phase 9 — `commit-tree`

**Upstream:** [`git/builtin/commit-tree.c`](git/builtin/commit-tree.c). **Docs:** `git/Documentation/git-commit-tree.adoc`. **Tests:** [`git/t/t1100-commit-tree-options.sh`](git/t/t1100-commit-tree-options.sh); also consumers in [`git/t/t0000-basic.sh`](git/t/t0000-basic.sh) and many scripts that build commits via plumbing.

- [x] **9.1** Build commit object: tree OID, parent(s), author/committer with **injected timestamps** (no hidden `SystemTime::now()` in library APIs per AGENT.md), encoding header, message from `-m` / `-F` / stdin.
- [x] **9.2** GPG signing hooks (`-S`) only if ported tests require; otherwise omit for v1 (current v1 behavior: `commit-tree` rejects `-S` / `--gpg-sign` as unsupported options).
- [x] **9.3** Write commit to object store and print hash to stdout.
- [x] **9.4** Port and pass `t1100-commit-tree-options.sh` and dependent basic flows.

---

## Phase 10 — `update-ref`

**Upstream:** [`git/builtin/update-ref.c`](git/builtin/update-ref.c). **Docs:** `git/Documentation/git-update-ref.adoc`. **Tests:** [`git/t/t1400-update-ref.sh`](git/t/t1400-update-ref.sh), [`git/t/t1404-update-ref-errors.sh`](git/t/t1404-update-ref-errors.sh). **Note:** upstream also has reftable backend; v1 may target **files** backend only if tests allow.

- [x] **10.1** Create/update/delete loose ref files under `refs/` and `HEAD` (symbolic and detached).
- [x] **10.2** Old-value verification (`<oldvalue>`), no-deref flags, `refs/heads/` vs full ref names.
- [x] **10.3** Batch stdin mode if required by `t1400`.
- [x] **10.4** Reflog append for updates when tests expect it (same format as Git).
- [x] **10.5** Port and pass `t1400-update-ref.sh` and error suite.

---

## Phase 11 — Integration, documentation parity, and cross-cutting tests

- [ ] **11.1** [`git/t/t1020-subdirectory.sh`](git/t/t1020-subdirectory.sh) — exercises `update-index`, `ls-files`, `cat-file`, `write-tree`, `checkout-index`, `read-tree` from subdirs.
- [ ] **11.2** [`git/t/t0000-basic.sh`](git/t/t0000-basic.sh) — broad smoke; port incrementally or subset by test groups.
- [x] **11.3** Ensure manpage / behavior parity checklist per command (read `git/Documentation/git-*.adoc` when marking a command complete).
- [ ] **11.4** **Logs** — per AGENT.md, one timestamped file under `logs/` per claimed task.
- [ ] **11.5** Final sweep: every `[x]` in this file has associated tests green under `./tests` with `gust` as the git substitute.

---

## Suggested implementation order (dependency-aware)

1. Phase 0 (workspace, objects, repo discovery)
2. Phase 1 `init`
3. Phases 2–3 `hash-object` + `cat-file`
4. Phase 4 index (`update-index` / `ls-files`)
5. Phase 5 `write-tree` → Phase 6 `ls-tree`
6. Phase 7 `read-tree` (largest)
7. Phase 8 `checkout-index`
8. Phase 9 `commit-tree`
9. Phase 10 `update-ref`
10. Phase 11 integration

Within each phase, prefer unlocking the **primary test script** listed first before expanding to optional scripts.

---

## Explicitly out of scope for v1 (from AGENT.md)

All porcelain commands (`commit`, `checkout`, `status`, `merge`, `clone`, …), transport, hooks beyond what `init` templates create, `gix`/`git2`, and any test not ported from `git/t/`.
