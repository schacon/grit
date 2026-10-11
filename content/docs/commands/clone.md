---
title: grit clone
summary: Copy a remote repository into a new directory.
group: Getting started
order: 2
---

## Synopsis

```text
grit clone <url> [<dir>]
```

## Description

Copies the repository at `<url>` into a new directory and checks out the remote's default branch. The remote is saved as `origin`, so [`grit fetch`](../fetch/), [`grit pull`](../pull/) and [`grit push`](../push/) work with no further setup.

Clone fetches every remote tag under `refs/tags/`, including tags on commits that are not reachable from any branch tip (matching `git clone`). Later [`grit fetch`](../fetch/) uses tag-following by default; pass `--tags` to fetch all tags again.

`<url>` can be an `https://` or `http://` URL, an `ssh://` URL or `user@host:path` address, a `git://` URL, a `file://` URL, a path to a local repository, or a path to a [git bundle](https://git-scm.com/docs/git-bundle) file (recognized by its header signature). A local path is stored as an absolute path so that later fetches work from anywhere.

Without `<dir>`, the directory is named after the last part of the URL with any `.git` suffix removed, so `https://github.com/gitbutlerapp/grit.git` clones into `grit`. The destination must not exist or must be empty. If the path already exists and is not empty, grit refuses the clone and leaves the directory and its files unchanged.

On an interactive terminal (human output, not `--json` or `--markdown`), grit prints a live **Checking out files** counter on stderr while the working tree is populated after the fetch completes.

For private GitHub repositories over HTTPS, sign in first with [`grit auth`](../auth/).

You can clone from a shallow repository (including one created with `git clone --depth`); grit copies the shallow boundary into the new repository so history matches the source.

## Options

| Option | Description |
| --- | --- |
| `<url>` | The repository to clone: a URL or a local path. |
| `<dir>` | The directory to clone into. Defaults to the repository's name. |

## Examples

Clone a repository from GitHub:

```console
$ grit clone https://github.com/gitbutlerapp/grit.git
Cloning into 'grit' ...
Receiving objects: 4.12 MiB, 8.50 MiB/s, done.
Cloned into 'grit' on branch main.
```

Progress lines are written to stderr and update in place on a terminal. When stderr is not a TTY (for example in a CI log or with stderr redirected), only the initial `Cloning into …` line is shown.

Clone into a directory with a different name:

```console
$ grit clone git@github.com:gitbutlerapp/grit.git grit-src
```

Clone a local repository:

```console
$ grit clone /srv/git/project.git
```

## JSON output

Pass `--json` for stable, scripting-friendly output:

| Field | Type | Meaning |
| ----- | ---- | ------- |
| `url` | string | The URL or path that was cloned. |
| `path` | string | Directory the clone was created in. |
| `branch` | string | Branch that was checked out. |

Example:

```json
{
  "url": "/srv/git/project.git",
  "path": "project",
  "branch": "main"
}
```

## See also

[grit init](../init/), [grit remote](../remote/), [grit fetch](../fetch/), [grit auth](../auth/)
