# t5602-clone-remote-exec

## Problem

`clone` with scp-style SSH URLs (`host:/path`) and `GIT_SSH` set did not invoke the wrapper before failing when the path did not resolve to a local repo. Upstream test `t5602` expects `GIT_SSH` to receive argv matching Git: `host git-upload-pack '/path'` (or custom `-u` upload-pack).

## Fix

- `ssh_transport.rs`: `sq_quote_shell_arg` (Git `sq_quote_buf` semantics including `!`), `unresolved_ssh_clone_invoke_git_ssh` runs `GIT_SSH host "<upload-pack> '<path>'"` when URL is unresolved and `GIT_SSH_COMMAND` is unset; non-zero exit propagates; zero exit falls through to existing bail.
- `clone.rs`: call the helper before `bail!` in `run_ssh_clone`.

## Verification

- `./scripts/run-tests.sh t5602-clone-remote-exec.sh` — 3/3 pass.
- `cargo test -p grit-lib --lib` — pass.
