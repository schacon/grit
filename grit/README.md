# grit-rs

A from-scratch reimplementation of the `git` command-line tool in Rust, built
on top of [`grit-lib`](https://crates.io/crates/grit-lib).

Grit aims to match Git's behavior closely enough that the upstream Git test
suite can be run against it. See the
[progress dashboard](https://schacon.github.io/grit) for current test pass
rates.

## Design philosophy

- **Behavioral compatibility with Git.** The same flags, the same output
  format, the same exit codes. Where Git's behavior is surprising, Grit
  reproduces the surprise.
- **No unsafe code.** The entire workspace forbids `unsafe`.
- **Fast startup.** Global options (`-C`, `--git-dir`, `--work-tree`, `-c`,
  `--bare`) are parsed by hand from argv before clap is invoked, so you only
  pay for the argument parser of the subcommand you actually run.

## Install

```sh
cargo install grit-rs
```

This puts a `grit` binary on your `$PATH`.

## Usage

`grit` is used exactly like `git`:

```sh
grit init myrepo
cd myrepo
echo "hello" > file.txt
grit add file.txt
grit commit -m "first commit"
grit log
```

## Implemented commands

Grit currently implements over 140 Git commands spanning porcelain, plumbing,
and network operations.

**Porcelain (everyday commands):**
add, am, archive, bisect, blame, branch, checkout, cherry-pick, clean, clone,
commit, config, describe, diff, fetch, format-patch, grep, init, log, merge,
mv, notes, pull, push, rebase, reset, restore, revert, rm, shortlog, show,
sparse-checkout, stash, status, submodule, switch, tag, worktree

**Plumbing (low-level):**
cat-file, check-attr, check-ignore, check-mailmap, check-ref-format, cherry,
commit-graph, commit-tree, count-objects, diff-files, diff-index, diff-pairs,
diff-tree, for-each-ref, hash-object, index-pack, interpret-trailers,
ls-files, ls-remote, ls-tree, merge-base, merge-file, merge-index,
merge-one-file, merge-tree, mktag, mktree, multi-pack-index, name-rev,
pack-objects, pack-refs, patch-id, prune, prune-packed, range-diff, read-tree,
reflog, rev-list, rev-parse, show-branch, show-index, show-ref, symbolic-ref,
unpack-file, unpack-objects, update-index, update-ref, update-server-info,
var, verify-commit, verify-pack, verify-tag, write-tree

**Network / transfer:**
bundle, daemon, fetch-pack, http-backend, http-fetch, http-push, receive-pack,
remote, send-pack, upload-archive, upload-pack

**Maintenance / utilities:**
backfill, bugreport, column, credential, credential-cache, credential-store,
diagnose, fast-export, fast-import, filter-branch, fmt-merge-msg, fsck, gc,
hook, mailinfo, mailsplit, maintenance, repack, replace, replay, rerere,
shell, stripspace, version, whatchanged

## Using as a library

The binary crate itself is not intended for library use. If you want to work
with Git repositories programmatically from Rust, depend on
[`grit-lib`](https://crates.io/crates/grit-lib) instead — it exposes the
full object model, diff engine, revision walker, index handling, and more.

## License

MIT
