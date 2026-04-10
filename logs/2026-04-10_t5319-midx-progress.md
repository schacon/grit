# t5319 multi-pack-index progress

## Fixes shipped

- **Clap panic**: Removed duplicate global `-C` on `git config` (`git_dir_path` now uses `--git-dir` only). Debug builds were panicking on every `grit` invocation.
- **Pack index v1**: `read_pack_index` now parses legacy v1 `.idx` (required for `pack-objects --index-version=1` in t5319).
- **MIDX write**: Include all `*.idx` under `pack/` (not only `pack-*.idx`); duplicate resolution matches Git (preferred pack, then newer mtime, then lower pack id); emit **LOFF** when offsets need the high bit; pad PNAM/BTMP per 4-byte alignment.
- **MIDX read path**: `try_read_object_via_midx` + `midx_oid_listed_in_tip`; `Odb` attaches `config_git_dir` and when `core.multiPackIndex` is true prefers MIDX for packed reads (and alternates when enabled).

## Still failing (major gaps)

t5319 still reports **~84/98** failures. Remaining work includes:

- `multi-pack-index` CLI: `--stdin-packs`, `--preferred-pack`, `--progress` / `--no-progress`, full **verify** (checksum, chunk validation, progress messages), **expire**, **repack** (`--batch-size`, delta islands), **bitmap** / `test-tool read-midx --show-objects` / `--bitmap` BTMP semantics.
- Error strings must match Git (`fatal: multi-pack-index …`) in many reader tests; current midx reader only covers a subset.
- `rev-list` / `count-objects` may need explicit MIDX-aware enumeration where they bypass `Odb::read`.

## Commands

```bash
cd tests && GUST_BIN=../target/debug/grit sh ./t5319-multi-pack-index.sh
```
