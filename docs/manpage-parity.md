# Manpage/Behavior Parity Checklist (v1 Plumbing)

This is a living checklist for reviewing `grit` against upstream Git docs and observed behavior.  
Status values are intentionally conservative until reviewed command-by-command.

| Command | Upstream doc path | Behavior areas to verify vs docs/tests | Reviewed vs `grit` |
|---|---|---|---|
| `init` | `git/Documentation/git-init.adoc` | Repo directory layout (`.git` / bare), templates, `HEAD` initialization, `--initial-branch`, `--shared`, `--separate-git-dir`, re-init messaging | ☐ Not reviewed |
| `hash-object` | `git/Documentation/git-hash-object.adoc` | stdin/file input modes, type selection, `-w` write semantics, `--stdin-paths`, `--path`, `--literally`, error handling for invalid paths/types | ☐ Not reviewed |
| `cat-file` | `git/Documentation/git-cat-file.adoc` | `-t`/`-s`/`-p`/existence modes, object type rendering, tag dereference behavior, batch modes (`--batch*`), malformed object/error output | ☐ Not reviewed |
| `update-index` | `git/Documentation/git-update-index.adoc` | add/remove/update paths, `--cacheinfo`/`--index-info`, refresh/racy stat handling, flag toggles (`assume-unchanged`, `skip-worktree` as applicable), index format compatibility | ☐ Not reviewed |
| `ls-files` | `git/Documentation/git-ls-files.adoc` | default index listing, status filters (`-o/-i/-m/-d/-k/-u`), stage output (`-s`), excludes/pathspec behavior, formatting (`-z`, `--format`, `--deduplicate`) | ☐ Not reviewed |
| `write-tree` | `git/Documentation/git-write-tree.adoc` | tree construction from index, sort/mode correctness, `--prefix`, `--missing-ok`, cache-tree interactions (if present), failure cases for unresolved entries | ☐ Not reviewed |
| `ls-tree` | `git/Documentation/git-ls-tree.adoc` | tree traversal depth/options (`-r/-d/-t`), path restriction semantics, output variants (`--name-only`, `--long`, `--format`), quoting/escaping behavior | ☐ Not reviewed |
| `read-tree` | `git/Documentation/git-read-tree.adoc` | single-tree reads, `-m` merge modes (2-way/3-way), `-u` worktree updates, `--reset`, `--prefix`, conflict staging semantics, D/F edge cases | ☐ Not reviewed |
| `checkout-index` | `git/Documentation/git-checkout-index.adoc` | checkout modes (`-a`, path list/stdin), force/dry-run/quiet behavior, `--prefix`, `--stage`, temp output (`--temp`/`--tmpdir`), symlink/platform behavior | ☐ Not reviewed |
| `commit-tree` | `git/Documentation/git-commit-tree.adoc` | commit object headers (tree/parent/author/committer), message sources (`-m`, `-F`, stdin), encoding/signing flags, timestamp/env handling, stdout hash output | ☐ Not reviewed |
| `update-ref` | `git/Documentation/git-update-ref.adoc` | create/update/delete refs, old-value verification, deref/no-deref behavior, batch stdin protocol, reflog writes, invalid refname/error semantics | ☐ Not reviewed |

## Review notes

- When a command is reviewed, change its status from `☐ Not reviewed` to one of:
  - `☑ Reviewed (matches in-scope behavior)`
  - `△ Reviewed (differences noted)`
- Record significant deltas and follow-up tasks here as short bullets, grouped by command.
