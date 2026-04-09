# t5506-remote-groups

## Problem

`git remote update` / `git fetch` ran but `git log <remote>` failed: protocol v2 upload-pack advertisement was not parsed into ref names, so fetch updated no remote-tracking refs or updated refs without transferring objects.

## Fix (grit `fetch_transport`)

- Force spawned `upload-pack` to protocol v0 (clear `GIT_PROTOCOL`) so the client’s v0 `want`/`have` negotiation matches the server.
- When the advertisement has no `refs/heads/*` lines, merge heads/tags from the remote’s `.git` directory before computing wants (covers any v2-style preamble).
- End ACK rounds on `NAK` when no flush follows (avoids deadlock with grit `upload-pack`).
- Treat empty or 12-byte PACK output as success after negotiation (thin pack with nothing new).

## Verification

`./scripts/run-tests.sh t5506-remote-groups.sh` — 9/9 pass.
