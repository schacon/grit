# Grit Code Review — v3

**Reviewed:** 2026-04-01
**Scope:** `grit-lib/src/` + `grit/src/commands/` + `grit/src/main.rs`

---

## Summary

The codebase has a strong foundation: good use of `thiserror`/`anyhow`, typed `ObjectId`/`ObjectKind`, proper lock-file writes in the ref layer, and solid index/diff/rev_parse library modules. However, there is a significant amount of copy-paste duplication that has already been copied at least twice (identity formatting, ref traversal, revision resolution) and will become exponentially worse as more commands are added. The most critical issue is that `state.rs` reimplements ref resolution logic already in `refs.rs`, creating a divergence that is invisible at compile time but will cause subtle bugs. Several architecture violations of the stated AGENT.md rules are present (stringly-typed fields, direct file writes bypassing library APIs, dead code). These should be addressed before adding more commands.

**Overall health: Fair.** The library layer is better than the command layer. The command layer is where most duplication lives, partly because `log.rs` was built without using `grit_lib::rev_list`.

---

## Category 1: Duplication — HIGH

### 1.1 — Ref resolution duplicated 4 ways

**Files:** `grit-lib/src/refs.rs`, `grit-lib/src/state.rs`, `grit/src/commands/log.rs`, `grit/src/commands/branch.rs`

`refs.rs` has the authoritative `resolve_ref` + `lookup_packed_ref`. `state.rs` at lines 197–246 has its own private `resolve_ref` + `resolve_packed_ref` that are functionally identical but return `Option<ObjectId>` instead of `Result<ObjectId>`. `log.rs:446–477` has `resolve_revision`, a simpler version that reads ref files directly via `std::fs::read_to_string` without using the library. `branch.rs:314–343` has `resolve_rev`, another duplicate that also reads ref files directly.

The library already exposes `rev_parse::resolve_revision` which handles full DWIM, peel, packed-refs, and abbreviated OIDs. All four local implementations should be deleted and replaced with calls to `grit_lib::rev_parse::resolve_revision`.

**Fix:** Delete `state::resolve_ref` / `state::resolve_packed_ref` and change `state::resolve_head` to call `refs::resolve_ref` for packed-ref lookups. Delete `log::resolve_revision` and `branch::resolve_rev`; use `grit_lib::rev_parse::resolve_revision` instead.

---

### 1.2 — packed-refs parser duplicated inside `state.rs`

**Files:** `grit-lib/src/refs.rs:101–122`, `grit-lib/src/state.rs:222–246`

`refs::lookup_packed_ref` and `state::resolve_packed_ref` are line-for-line duplicates except for the return type. `state.rs` uses `if let Some((hex, name)) = line.split_once(' ')` while `refs.rs` uses `let mut parts = line.splitn(2, ' ')`, but the semantics are identical.

**Fix:** `state::resolve_ref` should call `refs::resolve_ref` (or `refs::read_head`) directly rather than reimplementing it.

---

### 1.3 — Git identity formatting duplicated across `log.rs` and `show.rs`

**Files:** `grit/src/commands/log.rs:393–443`, `grit/src/commands/show.rs:459–507`

All four of these are identical functions, byte for byte:
- `extract_name(ident: &str) -> String`
- `extract_email(ident: &str) -> String`
- `format_ident_display(ident: &str) -> String`
- `format_date(ident: &str) -> String`

**Fix:** Move to a shared module in `grit-lib` — ideally as methods on a `GitIdent` or `Signature` newtype (see Missing Abstractions §4.1).

---

### 1.4 — `apply_format_string` duplicated across `log.rs` and `show.rs`

**Files:** `grit/src/commands/log.rs:284–391`, `grit/src/commands/show.rs:339–457`

Both files contain an `apply_format_string` that expand `%H`, `%h`, `%T`, `%t`, `%P`, `%p`, `%an`, `%ae`, `%ad`, `%ai`, `%cn`, `%ce`, `%cd`, `%ci`, `%s`, `%b`, `%n`, `%%`. The implementations are structurally identical; the only difference is that `log.rs` uses a local `CommitInfo` struct while `show.rs` works with a local `CommitInfo<'a>` holding borrows. Both work around the same underlying need: format placeholders applied to a `CommitData`.

**Fix:** Add `format_commit_str(template: &str, oid: &ObjectId, commit: &CommitData) -> String` to `grit-lib` (perhaps in `objects.rs` or a new `format.rs` module) and call it from both commands.

---

### 1.5 — `format_git_timestamp` and `ensure_trailing_newline` duplicated

**Files:** `grit/src/commands/commit.rs:308–314`, `grit/src/commands/tag.rs:449–455` (timestamp); `commit.rs:344–350`, `tag.rs:458–464` (trailing newline)

Both functions are character-for-character identical in the two files.

**Fix:** Move `format_git_timestamp` to `grit-lib` (already belongs there since it formats stored identity strings). Move `ensure_trailing_newline` to a shared CLI utility or inline the 2-line logic.

---

### 1.6 — Recursive ref-directory traversal duplicated 4 ways

**Files:** `grit-lib/src/refs.rs:246–274`, `grit/src/commands/branch.rs:276–304`, `grit/src/commands/tag.rs:256–284`, `grit/src/commands/log.rs:515–545`

All four functions walk a refs directory recursively, collect `(name, ObjectId)` pairs, and sort them. `refs::list_refs` / `refs::collect_refs` is the authoritative library version. `branch::collect_branches`, `tag::collect_tags`, and `log::collect_refs_from_dir` each reimplement this from scratch using direct `fs::read_to_string`.

`log::collect_refs_from_dir` is particularly fragile: it uses `full_ref.find(strip_prefix)` on the absolute path string — if the git-dir path happens to contain `refs/heads/` as a substring, it will produce wrong results.

**Fix:** All three command-side functions should call `refs::list_refs(&repo.git_dir, "refs/heads/")`, etc.

---

### 1.7 — Index "load or empty" pattern duplicated everywhere

**Files:** `add.rs:60–64`, `commit.rs:77–81`, `commit.rs:168–172`, `status.rs:59–63`, `rm.rs:59–63`

```rust
let index = match Index::load(&repo.index_path()) {
    Ok(idx) => idx,
    Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
    Err(e) => return Err(e.into()),
};
```

This 4-line pattern is used identically in 5 places.

**Fix:** Add `Index::load_or_new(path: &Path) -> Result<Index>` to the library (or at least a helper in a shared command utility module).

---

### 1.8 — Worktree directory-walk duplicated

**Files:** `grit/src/commands/add.rs:276–303`, `grit/src/commands/status.rs:276–319`

`add::walk_directory` and `status::walk_for_untracked` both recursively enumerate the working tree, skip `.git`, and produce relative-path lists. They differ only in whether they descend into dirs with tracked files.

**Fix:** Extract a shared `WorktreeWalker` or a library function `worktree_files(work_tree: &Path, include_dirs: bool)` in `grit-lib::diff` or a new `grit-lib::worktree` module.

---

## Category 2: Architecture — HIGH

### 2.1 — `state.rs` reimplements `refs.rs` internals

**File:** `grit-lib/src/state.rs:197–246`

`state.rs` contains a private `resolve_ref` + `resolve_packed_ref` pair that duplicates the logic in `refs.rs`. This means if `packed-refs` parsing is fixed or the format changes, it needs to be fixed in two places. `state.rs` is supposed to be about *what state the repo is in* (HEAD, in-progress operations), not about *how refs are read from disk*.

**Fix:** `state::resolve_head` should call `refs::read_head` (already public) and `refs::resolve_ref` for the OID lookup. The private `resolve_ref` / `resolve_packed_ref` in `state.rs` should be deleted.

---

### 2.2 — `main.rs` mutates global env to pass `GIT_DIR`

**File:** `grit/src/main.rs:138–140`

```rust
if let Some(git_dir) = &cli.git_dir {
    std::env::set_var("GIT_DIR", git_dir);
}
```

`std::env::set_var` is a global, unsynchronised mutation. Even in a single-threaded binary this is a code smell because it:
- Couples the CLI to the env-var convention for passing config to the library
- Makes the path non-obvious (set here, read in `Repository::discover`)
- Is incompatible with any future multi-threaded use (e.g. parallel test runners)

AGENT.md says "Explicit context: Time, randomness, and environment are **arguments or injectable providers**, not hidden `std::env` reads."

**Fix:** Add an `Options` struct (or parameters) to `Repository::discover` / `Repository::open` that accept an explicit `git_dir` override, and pass it from `main`.

---

### 2.3 — Commands bypass `refs::write_ref` with direct file writes

**Files:** `grit/src/commands/commit.rs:317–333`, `grit/src/commands/branch.rs:188–193`, `grit/src/commands/tag.rs:138–143`, `grit/src/commands/tag.rs:181–186`

`refs::write_ref` uses a lock file (`path.with_extension("lock")` + atomic rename) to ensure writes are safe under concurrent access. But several commands write ref files directly with `fs::write`, bypassing this safety mechanism.

**Fix:** All ref writes should go through `refs::write_ref`. For HEAD writes (in `commit.rs:325–329`) there should be a dedicated `refs::write_head` or the existing `write_ref` should handle `"HEAD"` correctly.

---

### 2.4 — `TagData.object_type` is `String` instead of `ObjectKind`

**File:** `grit-lib/src/objects.rs:395`

```rust
pub object_type: String,
```

AGENT.md: "Match on types, never strings." The `object_type` field stores `"commit"`, `"tree"`, `"blob"`, or `"tag"` — all values of the existing `ObjectKind` enum. Keeping it as `String` forces callers to do string comparisons.

**Fix:** Change `TagData.object_type` to `ObjectKind`. Update `parse_tag` to call `ObjectKind::from_bytes` and `serialize_tag` to call `.as_str()`.

---

### 2.5 — `DiffEntry.old_mode`/`new_mode` are `String` instead of `u32`

**File:** `grit-lib/src/diff.rs:75–78`

```rust
pub old_mode: String,
pub new_mode: String,
```

File modes are integers (e.g. `0o100644`). Storing them as pre-formatted strings merges the data model with the display concern. Every consumer that needs to compare or branch on modes must parse or match strings.

**Fix:** Change to `u32`. Consumers that need to display modes can format inline or via a helper.

---

### 2.6 — `log.rs` uses its own commit graph walk instead of `grit_lib::rev_list`

**File:** `grit/src/commands/log.rs:122–185`

`log.rs` has `walk_commits` — its own BFS/DFS over the commit graph with `--max-count`, `--skip`, and `--first-parent` support. The library already has `grit_lib::rev_list::rev_list` + `grit_lib::rev_list::render_commit` (used by `rev_list.rs`). Having two separate walkers means changes to ancestry walk logic (e.g. topo-order, date-order) will need to be made in two places.

**Fix:** Refactor `log.rs` to use `grit_lib::rev_list`. This may require extending `RevListOptions` with the decoration fields currently handled by `log.rs`.

---

### 2.7 — Dead code: `tag::resolve_head_oid`

**File:** `grit/src/commands/tag.rs:500–508`

```rust
#[allow(dead_code)]
fn resolve_head_oid(git_dir: &Path) -> Result<ObjectId> {
```

This function is unused. The `#[allow(dead_code)]` suppresses the compiler warning rather than removing the code.

**Fix:** Delete the function.

---

### 2.8 — `log.rs::collect_decorations` doesn't use `refs::list_refs`

**File:** `grit/src/commands/log.rs:480–545`

`collect_decorations` manually traverses `refs/heads/` and `refs/tags/` with its own recursive helper `collect_refs_from_dir` rather than calling `refs::list_refs`. The path-extraction logic on line 536 (`full_ref.find(strip_prefix)`) is fragile (see §1.6).

**Fix:** Use `refs::list_refs(&repo.git_dir, "refs/heads/")` and `refs::list_refs(&repo.git_dir, "refs/tags/")`.

---

## Category 3: Simplification — MEDIUM

### 3.1 — `status.rs::untracked` is a `String` not an enum

**File:** `grit/src/commands/status.rs:36`

```rust
pub untracked: String,
```

The field accepts `"no"`, `"normal"`, or `"all"`. This is compared with `args.untracked != "no"` (line 82), which will silently ignore any typo. An enum with `clap::ValueEnum` would give compile-time safety and proper help text.

**Fix:**
```rust
#[derive(Debug, Clone, clap::ValueEnum, PartialEq, Eq)]
pub enum UntrackedMode { No, Normal, All }
```

---

### 3.2 — `rm.rs::check_and_remove` returns stringly-typed errors

**File:** `grit/src/commands/rm.rs:150`

```rust
fn check_and_remove(...) -> std::result::Result<(), String>
```

Using `String` as the error type forces the caller to `eprintln!` manually and prevents using `?`. AGENT.md explicitly calls out stringly-typed errors as an anti-pattern.

**Fix:** Use `anyhow::Result` and return `Err(anyhow::anyhow!(...))`. The caller in `run` already uses `anyhow::Result`.

---

### 3.3 — Unreachable branch in `show::show_commit`

**File:** `grit/src/commands/show.rs:137–139`

```rust
Some(other) if other.starts_with("format:") || other.starts_with("tformat:") => {
    // Already handled above — unreachable
}
```

This match arm was added to suppress a compiler warning but is self-admittedly dead. The code around it should be restructured to eliminate the unreachable branch.

---

### 3.4 — `rev_list.rs::run` uses hand-rolled argument parsing

**File:** `grit/src/commands/rev_list.rs:30–100`

The command accepts `Vec<String>` args and manually parses them in a while loop. This bypasses clap entirely — there is no `--help` output for `rev-list` flags, error messages are ad-hoc, and adding new flags requires modifying the loop. The only justification would be if `rev-list` needed to support `--` passthrough, but that can be handled with clap's `trailing_var_arg`.

**Fix:** Define `rev-list` flags as proper `#[arg]` fields, or at least document why raw argv is needed here.

---

### 3.5 — `ObjectId::from_hex` is a trivial wrapper

**File:** `grit-lib/src/objects.rs:71–73`

```rust
pub fn from_hex(s: &str) -> Result<Self> {
    s.parse()
}
```

This is just `s.parse::<ObjectId>()` with an alias. It adds surface area with no benefit since `s.parse()` already works via the `FromStr` impl. However since it is part of the public API and widely used in commands, removing it would be disruptive; its continued existence should at least be a conscious decision.

---

### 3.6 — `objects.rs` doc comment misplacement

**File:** `grit-lib/src/objects.rs:63–75`

The `#[must_use]` attribute and doc comment for `from_hex` appear on lines 64–73, *between* `loose_suffix` and `from_hex` in source order. Visually, `loose_suffix` (line 75) appears to have no doc comment, and the `from_hex` doc appears as if it belongs to `loose_suffix`. This is a rendering bug in the source — the `#[must_use]` and doc block should immediately precede `from_hex`.

---

### 3.7 — `rm.rs` ignores `any_matched`

**File:** `grit/src/commands/rm.rs:136`

```rust
let _ = any_matched;
```

`any_matched` is set but then explicitly discarded. If it was meant to handle the `--ignore-unmatch` case it is now unused. This should either be removed or used.

---

## Category 4: Missing Abstractions — MEDIUM

### 4.1 — No `GitIdent`/`Signature` type for identity strings

**Files:** `log.rs`, `show.rs`, `commit.rs`, `tag.rs`

The format `"Name <email> timestamp tz"` is parsed in at least 4 command files to extract name, email, date. There is no library type for this. As a result `extract_name` / `extract_email` / `format_date` are duplicated (§1.3).

**Fix:** Add a `Signature` struct to `grit-lib::objects` (or a new `grit-lib::ident` module):
```rust
pub struct Signature {
    pub name: String,
    pub email: String,
    pub timestamp: i64,
    pub tz_offset: String,
}
impl Signature {
    pub fn parse(s: &str) -> Result<Self> { ... }
    pub fn to_raw(&self) -> String { ... }
}
```
`CommitData.author` and `CommitData.committer` could then be `Signature` instead of raw `String`.

---

### 4.2 — No `FileMode` newtype for `u32` modes

**Files:** `diff.rs`, `index.rs`, `objects.rs`, `add.rs`, `commit.rs`, various commands

File modes (`0o100644`, `0o040000`, `0o120000`, etc.) are used as bare `u32` throughout with no helpers. Consumers must remember whether to format as `"%06o"` or `"%o"` (with the Git special-case for trees). The tree entry comparator and the mode-string formatter are separate utility functions rather than methods on a type.

**Fix:** Add a `FileMode(u32)` newtype with `is_tree()`, `is_blob()`, `is_symlink()`, `is_executable()` predicates and a `Display` impl that matches Git's formatting rules.

---

### 4.3 — `glob_matches` is private to `tag.rs`

**File:** `grit/src/commands/tag.rs:469–497`

Glob matching is needed by `tag --list` and `for-each-ref` (which has its own pattern matching). The implementation should live in the library (`grit-lib`) or at minimum in a shared command utility.

---

### 4.4 — No shared `flatten_tree` utility

**File:** `grit/src/commands/rm.rs:275–304`

`rm.rs::flatten_tree_to_map` builds a `path → ObjectId` map by recursively walking a tree. The same operation will be needed by `checkout`, `merge`, `diff`, etc. This belongs in `grit-lib` (perhaps on `Odb` or in a `tree_util` module).

---

## Category 5: API Design — LOW

### 5.1 — `Repository` fields are all `pub` without accessor methods

**File:** `grit-lib/src/repo.rs:27–34`

`git_dir`, `work_tree`, and `odb` are all `pub`. AGENT.md says to default to `pub(crate)` and expose only supported API. Making `odb` directly pub means `Odb` is implicitly part of `Repository`'s public API surface. If the storage backend is ever changed, this is a breaking change. Accessor methods (`repo.odb()`, `repo.git_dir()`) would let the implementation evolve without breaking callers.

---

### 5.2 — `write_tree_from_index` is a free function in its own module

**File:** `grit-lib/src/write_tree.rs`

Per AGENT.md: "Keep 'plumbing' operations as **coherent methods** on the appropriate type (`Index::write_tree`) rather than a flat bag of free functions." `write_tree_from_index` logically belongs as `Index::write_tree(&self, odb: &Odb) -> Result<ObjectId>`.

---

### 5.3 — `init_repository` lives in `repo.rs` rather than being a `Repository` constructor

**File:** `grit-lib/src/repo.rs:196–249`

`init_repository` is a free function that returns a `Repository`. It would be more idiomatic as `Repository::init(path, options) -> Result<Self>`, consistent with `Repository::open` and `Repository::discover`.

---

## Summary Table

| # | Finding | Location | Priority |
|---|---------|----------|----------|
| 1.1 | Ref resolution duplicated 4 ways | `state.rs`, `log.rs`, `branch.rs` | **HIGH** |
| 1.2 | packed-refs parser duplicated in `state.rs` | `state.rs` vs `refs.rs` | **HIGH** |
| 1.3 | Identity formatting functions duplicated | `log.rs` + `show.rs` | **HIGH** |
| 1.4 | `apply_format_string` duplicated | `log.rs` + `show.rs` | **HIGH** |
| 1.5 | `format_git_timestamp` / `ensure_trailing_newline` duplicated | `commit.rs` + `tag.rs` | **HIGH** |
| 1.6 | Recursive ref-dir traversal duplicated 4 ways | `branch.rs`, `tag.rs`, `log.rs` | **HIGH** |
| 1.7 | Index "load or empty" duplicated 5× | `add.rs`, `commit.rs`×2, `status.rs`, `rm.rs` | **HIGH** |
| 1.8 | Worktree walk duplicated | `add.rs`, `status.rs` | **MEDIUM** |
| 2.1 | `state.rs` reimplements `refs.rs` internals | `state.rs` | **HIGH** |
| 2.2 | `main.rs` mutates env to pass `GIT_DIR` | `main.rs` | **HIGH** |
| 2.3 | Commands bypass `refs::write_ref` | `commit.rs`, `branch.rs`, `tag.rs` | **HIGH** |
| 2.4 | `TagData.object_type` is `String` | `objects.rs` | **HIGH** |
| 2.5 | `DiffEntry.old/new_mode` are `String` | `diff.rs` | **HIGH** |
| 2.6 | `log.rs` has its own commit walk | `log.rs` | **MEDIUM** |
| 2.7 | Dead code: `tag::resolve_head_oid` | `tag.rs` | **LOW** |
| 2.8 | `collect_decorations` doesn't use `refs::list_refs` | `log.rs` | **MEDIUM** |
| 3.1 | `status.untracked` is `String` | `status.rs` | **MEDIUM** |
| 3.2 | `rm::check_and_remove` returns `String` error | `rm.rs` | **MEDIUM** |
| 3.3 | Unreachable branch in `show_commit` | `show.rs` | **LOW** |
| 3.4 | `rev_list.rs` hand-rolls argument parsing | `rev_list.rs` | **MEDIUM** |
| 3.5 | `ObjectId::from_hex` redundant with `FromStr` | `objects.rs` | **LOW** |
| 3.6 | Doc comment misplaced in `objects.rs` | `objects.rs` | **LOW** |
| 3.7 | `any_matched` discarded with `let _ = ` | `rm.rs` | **LOW** |
| 4.1 | No `Signature`/`GitIdent` type | library | **MEDIUM** |
| 4.2 | No `FileMode` newtype | library | **MEDIUM** |
| 4.3 | `glob_matches` private to `tag.rs` | `tag.rs` | **MEDIUM** |
| 4.4 | No shared `flatten_tree` utility | `rm.rs` | **MEDIUM** |
| 5.1 | `Repository` fields all `pub` | `repo.rs` | **LOW** |
| 5.2 | `write_tree_from_index` should be `Index::write_tree` | `write_tree.rs` | **LOW** |
| 5.3 | `init_repository` should be `Repository::init` | `repo.rs` | **LOW** |

---

## Recommended Fix Order

Address HIGH items before adding more commands:

1. **§2.1 / §1.2** — Delete `state::resolve_ref` / `state::resolve_packed_ref`, use `refs::resolve_ref`.
2. **§2.3** — Route all ref writes through `refs::write_ref`.
3. **§1.1** — Delete `log::resolve_revision` and `branch::resolve_rev`; use `rev_parse::resolve_revision`.
4. **§1.6 / §2.8** — Replace `branch::collect_branches`, `tag::collect_tags`, `log::collect_refs_from_dir` with `refs::list_refs`.
5. **§4.1** — Add `Signature` type to library; update `CommitData`/`TagData`.
6. **§2.4 / §2.5** — Change `TagData.object_type` to `ObjectKind`; change `DiffEntry.old/new_mode` to `u32`.
7. **§1.3 / §1.4** — Move identity formatting and `apply_format_string` into the library.
8. **§1.7** — Add `Index::load_or_new`.
9. **§2.2** — Pass `GIT_DIR` explicitly to `Repository::discover`.
10. **§1.5** — Move timestamp/trailing-newline helpers out of commands.
