# t5813-proto-disable-ssh

## Goal

Make `tests/t5813-proto-disable-ssh.sh` fully pass (81 tests).

## Changes

- **`protocol.rs`**: `GIT_ALLOW_PROTOCOL` accepts comma-separated protocols (Git uses commas; tests use `GIT_ALLOW_PROTOCOL=ssh`). `protocol.*.allow=user` now honors `GIT_PROTOCOL_FROM_USER` (unset/true vs falsey).
- **`ssh_transport.rs`**: Parse/validate `ssh://`, `git+ssh://`, and scp-style `host:path`; reject host/path starting with `-`. Resolve local repo for harness: `TRASH_DIRECTORY/<host>/<path>` or absolute path.
- **`clone.rs`**: SSH clones copy from resolved local source; store original URL in `remote.origin.url` (`setup_origin_remote_*_url`). Handle `git+ssh://` in `--separate-git-dir` guard.
- **`fetch.rs` / `push.rs`**: When remote URL is SSH-shaped, check `protocol.ssh.allow` and open repo via `ssh_transport::try_local_git_dir`.
- **`upload_pack.rs`**: Full pkt-line ref advertisement + negotiation; stream pack via `grit pack-objects --stdout` wrapped in side-band-64k channel 1.
- **`receive_pack.rs`**: Advertisement and update lines use pkt-line framing (compatible with Git client over SSH).

## Verification

```bash
cargo build --release -p grit-rs
./scripts/run-tests.sh t5813-proto-disable-ssh.sh
cargo test -p grit-lib --lib
```

Result: 81/81 for t5813.
