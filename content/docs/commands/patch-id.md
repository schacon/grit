---
title: grit patch-id
summary: Stable patch-IDs for commits (cherry-pick equivalence).
group: Plumbing
order: 2
---

## Synopsis

```text
grit patch-id <REV|RANGE>...
grit patch-id --unstable <REV|RANGE>...
```

## Description

Prints one line per commit that has a defined patch-ID: `<patch-id> <commit>`, matching `git show --format= --no-ext-diff -U3 <REV> | git patch-id --stable` on the commit's tree diff. Text hunks include three unchanged context lines on each side; nearby context contributes to the ID, while unchanged lines outside the hunks do not. Merge commits and commits with no file changes are omitted.

A range argument expands to one line per commit, in the same order as [`grit revs`](../revs/):

- `A..B` lists the commits reachable from `B` but not from `A`, such as the commits of a branch since it left `main`.
- `A...B` lists the commits on both sides of the symmetric difference as one sequence, without saying which side each came from. To compare two versions of a rewritten branch, use [`grit revs --cherry-mark A...B`](../revs/), which pairs the sides by patch-ID.

Any other argument names one commit. Output follows argument order, so a commit named twice is printed twice.

Uses [`Repository::patch_ids_for_revs`](../../library/maintenance.md) with stable accumulation (file order independent). Pass `--unstable` for the legacy Git 1.9 algorithm.

### Benchmarking

Compare the same commit with the installed system Git:

```bash
/usr/bin/time -v grit patch-id HEAD --json
/usr/bin/time -v sh -c 'git show --format= --no-ext-diff -U3 HEAD | git patch-id --stable'
```

Use a large text file with sparse edits to measure hunk traversal rather than only process startup. Record wall time and peak resident memory with warm object caches; use the `dist` profile when building grit locally.

Measured on 2026-10-26 on Linux x86_64 (4 vCPUs, AMD EPYC 9V74), using a 14 MB file with 200,000 unique lines and ten replacements spaced 20,000 lines apart. Both grit binaries use the `dist` profile. Results are medians of 21 interleaved warm-cache runs per command after one warm-up; peak RSS comes from `/usr/bin/time -v` for each invocation.

| Implementation | Wall time | Median peak RSS | Peak RSS range |
| --- | ---: | ---: | ---: |
| System Git 2.55.0 | 62.91 ms | 62,604 KiB | 61,696–63,240 KiB |
| Grit before fix (`fa6a9fd55`) | 175.43 ms | 100,732 KiB | 100,008–101,892 KiB |
| Grit with three context lines | 146.02 ms | 101,632 KiB | 100,800–101,988 KiB |

The fix reduces median wall time by 17%. Median peak RSS differs by 900 KiB (0.9%), with overlapping observed ranges. The fixed result matches system Git's patch-ID for a normal unified diff on this fixture.

Ranges hash their commits on worker threads. Measured on 2026-10-27 on Linux x86_64 (4 vCPUs) with system Git 2.43.0 and a `release` build of grit, in a packed repository with a commit-graph: 5,000 base commits, a 100-commit branch rebased onto 200 new upstream commits (`old...new` has 400 commits). Means of 30 warm runs (hyperfine):

| Command | Wall time | Peak RSS |
| --- | ---: | ---: |
| `git rev-list old...new \| git diff-tree --stdin -p \| git patch-id --stable` | 20.3 ms | — |
| `grit patch-id old...new` | 17.6 ms | 23,576 KiB |
| `git rev-list --left-right --cherry-mark old...new` | 10.2 ms | 8,392 KiB |
| `grit revs --cherry-mark old...new` | 22.9 ms | 26,444 KiB |

Git's cherry-mark compares a cheap file-header id before computing a full patch-ID; grit computes the full ID for every commit, which accounts for most of the remaining gap.

## Options

| Option | Description |
| --- | --- |
| `--unstable` | Use unstable patch-ID hashing instead of stable (default). |

## Examples

```console
$ grit patch-id HEAD~2 HEAD~1 HEAD
a1b2c3d4e5f6789012345678901234567890abcd 0123456789abcdef0123456789abcdef01234567
fedcba0987654321fedcba0987654321fedcba09 abcdef0123456789abcdef0123456789abcdef01
```

Patch-IDs of a branch's commits since it left `main`, newest first (a rebase or a reworded message keeps each ID):

```console
$ grit patch-id main..feature
bbbce41090c2fd38689e39dda9f033194ea6b8cb 09d5318d2c9bb2881fd2b503861aadf43737463e
efc092c14f58acc31f0e129f61815ec4caccc849 a41d2d81bcbea60b67ac7f6c530d71bea889b628
9b734e090c6e3f7098bbf1fb6129bc3eeefb785b e778fe04f49705176ea24d7fa28182952a835187
```

## JSON output

Schema: [v1](/schemas/patch-id/v1.json)

| Field | Type | Meaning |
| ----- | ---- | ------- |
| `entries` | array | Each item has `patch_id` and `commit` (full hex object ids), in argument order with ranges expanded. |

Example:

```json
{
  "entries": [
    {
      "patch_id": "a1b2c3d4e5f6789012345678901234567890abcd",
      "commit": "0123456789abcdef0123456789abcdef01234567"
    }
  ]
}
```

## Markdown output

Schema: [v1](/schemas/patch-id/v1.json)

`--markdown` prints a table of patch IDs and commits.

```text
## Patch IDs

| Patch ID | Commit |
| --- | --- |
| `db20cbf19404d5a762136b964e1df57a06af6104` | `83352a61f6715ec3f0a297c6d6150725154fb833` |
```

## Exit status

| Code | Meaning |
| ---: | --- |
| `0` | Success ([`success`](../exit-codes/)). |
| `1` | Command failed ([`failure`](../exit-codes/)). |
| `2` | Invalid flags or arguments ([`usage`](../exit-codes/)). |

See [Exit codes](../exit-codes/) for shared rules and structured `--json` `kind` values.

## See also

- [Library maintenance — patch IDs](../../library/maintenance.md)
- [`grit revs --cherry-mark`](revs.md)
- [`grit pick`](pick.md)
