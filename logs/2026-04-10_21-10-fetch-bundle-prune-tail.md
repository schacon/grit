## 2026-04-10 — fetch bundle + prune tail iteration

### Scope worked

- Continued `t5510-fetch.sh` parity work from 192/215 baseline cluster.
- Focused on bundle/fetch interactions, write-commit-graph behavior, CLI refspec source resolution, and prune display parity.

### Code changes

- `grit/src/commands/fetch.rs`
  - improved CLI source ref resolution:
    - empty source (`:dst`) resolves via `HEAD`,
    - disambiguation now includes `refs/<name>`, `refs/tags/<name>`, `refs/heads/<name>`, `refs/remotes/<name>`, `refs/remotes/<name>/HEAD`.
  - fixed branch safety behavior around fetching into checked-out branch when source is explicit `HEAD` mapping.
  - added post-fetch commit-graph write hook honoring `fetch.writeCommitGraph`.
  - enabled path-based bundle-file fetches by invoking `bundle unbundle` for explicit bundle paths.
  - separated fetch display URL for terminal `From ...` output from FETCH_HEAD URL formatting.

- `grit/src/fetch_transport.rs`
  - refined CLI want resolution for upload-pack negotiation:
    - resolves source refs against both advertised refs and on-disk remote refs for local transports,
    - supports `:dst` and short remote-name disambiguation cases.

- `grit/src/commands/bundle.rs`
  - added `bundle create --version=3` parsing and v3 header output (`# v3 git bundle`, `@object-format=sha1`).
  - included prerequisite commit subjects in bundle header lines (`-<oid> <subject>`).
  - normalized `bundle list-heads` output to canonical full refs for heads/tags.
  - improved `-<n>` object selection to avoid packing unchanged parent-tree payload objects.

### Validation

- `cargo fmt` ✅
- `cargo check -p grit-rs` ✅
- `cargo test -p grit-lib --lib` ✅
- `cargo build --release -p grit-rs` ✅
- `./scripts/run-tests.sh t5510-fetch.sh`:
  - progressed through this iteration to **209/215**.

### Current remaining t5510 failures

- `187`, `190`, `192`, `193`, `194`, `196`

