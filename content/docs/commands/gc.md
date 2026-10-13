---
title: grit gc
summary: Repack the object database and prune unreachable loose objects.
group: Maintenance
order: 3
---

## Synopsis

```text
grit gc
grit gc --dry-run
grit gc --quiet
```

## Description

Runs repository garbage collection in-process:

1. Expire reflogs using `gc.reflogExpire*` rules (same layering as Git).
2. Repack all reachable objects into one pack ([`grit repack`](repack.md)).
3. Delete loose objects that are not reachable from any ref.

This is the usual way to shrink a `.git` directory after many local commits. It does not shell out to the system `git` binary.

## Options

| Option | Description |
| --- | --- |
| `--dry-run` | Run reflog expiry and repack in dry-run mode; no objects or refs are modified. |
| `-q`, `--quiet` | Suppress the human summary. |

## Examples

```console
$ grit gc
Packed 412 objects into .git/objects/pack/pack-….pack.
Pruned 0 unreachable loose objects (0 kept).
```

Dry run:

```console
$ grit gc --dry-run
Dry run: no changes written.
Would pack 412 reachable objects.
```

## JSON output

Schema: [v1](/schemas/gc/v1.json)

Pass `--json` for stable, scripting-friendly output:

| Field | Type | Meaning |
| ----- | ---- | ------- |
| `dry_run` | boolean | Whether this run only estimated work. |
| `reflog_entries_pruned` | number | Reflog entries removed (or that would be removed on dry run). |
| `repack` | object | Same fields as [`grit repack`](repack.md) JSON (`loose_before`, `packs_before`, `objects_packed`, `pack_bytes`, `pack_path`, `removed_pack_files`, `pruned_loose`). |
| `prune` | object | `{ "pruned": number, "kept": number }` unreachable loose-object counts. |

Example:

```json
{
  "dry_run": false,
  "reflog_entries_pruned": 0,
  "repack": {
    "loose_before": 380,
    "packs_before": 0,
    "objects_packed": 412,
    "pack_bytes": 88234,
    "pack_path": ".git/objects/pack/pack-abc123….pack",
    "removed_pack_files": 0,
    "pruned_loose": 380
  },
  "prune": {
    "pruned": 0,
    "kept": 0
  }
}
```

## Markdown output

Pass `--markdown` for an agent-friendly report with repack and prune sections.

Schema: [v1](/schemas/gc/v1.json)

## See also

- [`grit repack`](repack.md)
- [`grit fsck`](fsck.md)
