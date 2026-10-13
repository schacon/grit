---
title: grit fsck
summary: Check refs, objects, and pack indexes for problems.
group: Maintenance
order: 4
---

## Synopsis

```text
grit fsck
grit fsck --strict
grit verify
```

## Description

Verifies repository integrity without invoking the system `git` binary:

- Reference database checks ([`refs_fsck`](../../library/repository.md) parity).
- Reachability walk from ref roots with object header validation.
- Multi-pack-index verification when `objects/pack/multi-pack-index` exists.

`grit verify` is an alias for `grit fsck`. The command exits with status **1** when any error-severity issue is found (warnings alone still exit 0 unless `--strict` promotes them for refs).

## Options

| Option | Description |
| --- | --- |
| `--strict` | Treat ref-fsck warnings as errors. |

## Examples

Clean repository:

```console
$ grit fsck
Repository integrity check passed.
```

Report problems:

```console
$ grit fsck
error: abcd1234…: missingObject: object abcd1234… is missing
Integrity check failed.
```

## JSON output

Schema: [v1](/schemas/fsck/v1.json)

Pass `--json` for stable, scripting-friendly output:

| Field | Type | Meaning |
| ----- | ---- | ------- |
| `ok` | boolean | `true` when no error-severity issues were found. |
| `issues` | array | Each entry has `severity` (`error` or `warning`), `path`, `msg_id`, and `detail`. |

Example:

```json
{
  "ok": true,
  "issues": []
}
```

Example with a problem:

```json
{
  "ok": false,
  "issues": [
    {
      "severity": "error",
      "path": "deadbeef…",
      "msg_id": "missingObject",
      "detail": "object deadbeef… is missing"
    }
  ]
}
```

## Markdown output

Pass `--markdown` for a table of issues plus the `ok` flag.

Schema: [v1](/schemas/fsck/v1.json)

## See also

- [`grit gc`](gc.md)
- [`grit bundle verify`](bundle.md)
