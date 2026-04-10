# Plan: Make `grit status` fast (vs Git’s C implementation)

This document summarizes **why `grit status` is slow**, how that differs from **upstream Git**, and a **dependency-ordered** plan to fix it—with emphasis on **skipping ignored trees**, **avoiding redundant work**, and **index/stat fast paths**.

---

## 1. What `grit status` does today (hot path)

`grit/src/commands/status.rs` roughly:

1. **Load index** (sparse placeholders expanded).
2. **`diff_index_to_tree`** — staged changes (index vs `HEAD` tree).
3. **`diff_index_to_worktree`** — unstaged changes (index vs working tree files).
4. **`detect_renames`** (optional) on staged and unstaged entries.
5. **Untracked + ignored**: `collect_untracked_and_ignored` (full worktree walk + ignore checks).
6. Optionally **`refresh_untracked_cache_for_status`** when `core.untrackedCache` is enabled — **another** directory walk to maintain the index’s UNTR extension.

Output is filtered by **pathspec** only **after** steps 2–5 (see `status_path_matches`), so **pathspecs do not shrink the amount of filesystem or hashing work** today.

---

## 2. Main performance gaps vs Git

### 2.1 Unstaged diff: hash **every** tracked file (`diff_index_to_worktree`)

**Grit (`grit-lib/src/diff.rs`):** For each normal tracked entry, after `stat` and mode-only checks, the code **always** calls `hash_worktree_file` (read file + clean/smudge-related hashing). Comments state that hashing cannot be skipped when stat matches, to handle same-size edits and “racy” timestamps.

**Git (`wt-status.c` / `read-cache.c`):** Uses **cached `stat` data stored in the index** (`ce_stat_data`). When `lstat` matches that cache, Git treats the path as **unchanged** and **does not re-hash** the file. There is a separate **“racy Git”** rule: if the file’s mtime equals the index’s “racy timestamp” window, Git may still re-stat or re-check so correctness is preserved without hashing **everything** on every run.

**Impact:** In a large repo with a warm index and few edits, **Git is ~O(changed files)** for content checks; **Grit is ~O(all tracked files)** full reads—often the **dominant** cost.

---

### 2.2 Double work: untracked cache refresh **and** a second full untracked walk

When `core.untrackedCache` is enabled, `refresh_untracked_cache_for_status` walks the tree (or uses valid cache nodes) to update the **UNTR** structure written back into the index.

Immediately after, **`collect_untracked_and_ignored`** runs a **separate** `visit_untracked_node` / `fs::read_dir` traversal to produce the actual untracked/ignored lists. The refreshed cache is **not** used as the source of truth for those paths.

**Git:** `read_directory` / `dir.c` drives **one** traversal that both **updates optional untracked cache state** and **feeds `wt-status`**.

**Impact:** Up to **2×** directory enumeration on many runs.

---

### 2.3 Ignore matching: per-path cost (`IgnoreMatcher::check_path`)

**Grit (`grit-lib/src/ignore.rs`):** `rules_for_path` loads **per-directory** `.gitignore` into `gitignore_cache` (good), but then **builds a new `Vec` of rules** by **cloning** rules from **every ancestor** for **every** `check_path` call. Each untracked file triggers this.

**Git:** Uses layered exclude machinery with **compiled** patterns and **directory** early-exit (see below); avoids re-allocating full rule lists per path.

**Impact:** **O(files × rules)** cloning and matching; noticeable in trees with many small files under nested `.gitignore` layers.

---

### 2.4 Ignored directories: partial early exit only in some modes

**Grit (`visit_untracked_directory` in `status.rs`):** There **is** a useful short-circuit for **`--ignored=matching`** + **`--untracked-files=all`**: if the directory is excluded, emit `dir/` and **do not recurse**.

For **default** untracked modes (`normal` / default showUntrackedFiles), the walk still tends to **recurse into subtrees** to implement collapse rules, and **`traditional_normal_directory_only`** can perform an **extra full subtree scan** (explicit stack + `read_dir`) to decide whether to print a single ignored directory line.

**Git `dir.c`:** **Untracked cache** + **“path is excluded and no tracked files inside”** pruning avoids descending into large ignored trees (e.g. `node_modules/`, `target/`) in many cases.

**Impact:** Huge ignored subtrees still cost **many** `read_dir` + `check_path` calls unless modes hit the narrow fast paths.

---

### 2.5 Rename detection (`detect_renames`)

**Grit:** Builds a **full score matrix** of deleted × added pairs; reads **all** deleted and added blob contents; **O(D × A)** similarity work.

**Git:** More sophisticated pairing (basename priority, limits, sometimes binary heuristics) and long-standing optimizations; still potentially heavy, but Grit’s **naïve full matrix** is a worst case.

**Impact:** Bad when many adds/deletes (rebases, large refactors).

---

### 2.6 Pathspecs do not limit I/O

Pathspecs only **filter printed paths** after diffs and untracked collection. **Git** narrows work when pathspecs are present (fewer stats / less traversal where applicable).

**Impact:** `grit status -- path` still pays **full-repo** costs.

---

### 2.7 Ancillary costs

- **`has_symlink_in_path`:** Per index entry, walks path components with **`symlink_metadata`** — **O(entries × depth)** syscalls.
- **`diff_index_to_worktree`:** Loads **`ConfigSet`** once; loads **root-only** `load_gitattributes(work_tree)` — not full nested attributes (separate correctness topic), but `get_file_attrs` scans **all** rules per path.
- **Staged diff + submodule / symlink / intent-to-add** branches:** Extra stats and reads as expected; same class as Git.

---

## 3. Plan (dependency order)

### Phase A — Correctness-preserving fast path for unstaged **clean** files (highest ROI)

**A.1** When `stat_matches(ie, &meta)` is **true** (size, mtime, ctime, dev, ino per index), **treat as unchanged** and **skip** `hash_worktree_file`, **unless** racy-git conditions require a second check (port Git’s `racy_git_is_fickle` / equivalent: mtime in the same second as index, etc.).

**A.2** Optionally: if stat matches but flags suggest possible raciness, **only then** re-hash or use a cheaper check.

**Depends on:** Index entries actually populated with **trustworthy** stat fields (they usually are after `git add` / `git status` under Git; verify Grit’s index fill paths).

---

### Phase B — Single untracked / ignored traversal (or reuse UC output)

**B.1** Either:

- **Merge** `refresh_untracked_cache_for_status` and `collect_untracked_and_ignored` into **one** `read_directory`-style pass that produces **both** UNTR update data **and** user-visible untracked/ignored lists, **or**

- After refresh, **derive** untracked/ignored lists from the **UntrackedCache tree** + same ignore rules (must match Git’s ordering and collapse rules exactly).

**B.2** Ensure **ignored directory** pruning matches Git: **do not** `read_dir` inside a directory that is **fully ignored** and **contains no tracked paths**, for the relevant `--untracked-files` / `--ignored` combinations.

---

### Phase C — Ignore engine efficiency

**C.1** Replace “clone all rules for every path” with:

- **Precomputed** stacked rule lists per **directory inode** (or per `gitignore` scope), or

- **Compile** patterns once per loaded file (Git uses `wildmatch` on compiled state).

**C.2** **Directory** checks: cheap “is this path excluded as a directory?” to **prune** before file-level checks.

---

### Phase D — Pathspec-scoped work

**D.1** Thread pathspecs into:

- `diff_index_to_worktree` (only visit index entries under pathspec),

- untracked collection (only walk subtrees intersecting pathspec),

- optional: staged diff similarly.

Match Git’s behavior for **exclude** pathspecs and **magic** pathspecs.

---

### Phase E — Rename detection budget

**E.1** Cap candidate pairs, prefer **same-basename** first (Grit already sorts scores this way partially).

**E.2** Skip similarity for **size** / **binary** mismatches where safe.

**E.3** Align with `diff.renameLimit` / `status.showUntrackedFiles`–style limits if Git exposes them for status.

---

### Phase F — Smaller wins

**F.1** Cache `has_symlink_in_path` results per **parent directory** prefix.

**F.2** Avoid **double** `fs::read` in `hash_worktree_file` + raw OID fallback where one read can supply both.

**F.3** **`GIT_TRACE2_PERF`**: extend timing around **diff**, **untracked**, **ignore** to validate wins (some trace hooks exist for read_directory).

---

## 4. How this relates to “ignoring ignored paths”

Improving performance is **not** only “apply `.gitignore` earlier” (though **directory-level exclude** is critical). The largest win for typical repos is usually:

1. **Stat cache fast path** (Phase A) — skip hashing **clean** tracked files.

2. **One walk** + **ignored tree pruning** (Phases B & C) — skip **enumerating** `node_modules/`-scale trees.

3. **Cheaper ignore matching** (Phase C) — per-file work proportional to **depth**, not **rules × files** with allocations.

---

## 5. Suggested validation

- **Large repo, clean tree:** `grit status` time should approach **Git** (dominated by stat + one directory scan).

- **Huge ignored tree:** time should **not** scale with file count inside ignored dirs when untracked mode allows pruning.

- **Pathspec:** `grit status -- subdir` should scale with **subdir**, not full tree.

- Existing harness: **`t7508`**, **`t7063`** (untracked cache), **`t0008`** / ignore tests after refactors.

---

## 6. Summary

| Issue | Grit | Git (typical) |
|-------|------|----------------|
| Unstaged clean files | Hash/read all | Stat vs index → skip hash |
| Untracked | Often **two** walks + per-file ignore vec build | One integrated walk + prune |
| Ignored dirs | Limited short-circuit | Strong directory exclude + UNTR |
| Pathspec | Filter output only | Narrow work |
| Renames | Dense D×A matrix | Bounded / optimized pairing |

**Priority:** **Phase A** (stat fast path) and **Phase B/C** (single walk + directory exclude) deliver the largest user-visible speedups; **Phase D** helps narrow `status path` workflows.

---

*Based on reading `grit/src/commands/status.rs`, `grit-lib/src/diff.rs`, `grit-lib/src/ignore.rs`, `grit-lib/src/untracked_cache.rs`, and comparison to Git’s `wt-status.c` / `dir.c` behavior.*
