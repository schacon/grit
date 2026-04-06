## 2026-04-06 — t6001-rev-list-graft

### Scope
- Claimed `t6001-rev-list-graft` as the active Rev Machinery target.

### Baseline reproduction
- Direct:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6001-rev-list-graft.sh`
  - Baseline: **3/14 pass**.

### Initial failure clusters
- Graft semantics missing:
  - `rev-list` output ignores `.git/info/grafts`, so grafted parent chains are not traversed in `basic`, `--parents`, and `--parents --pretty=raw` checks.
- Path limiter disambiguation missing:
  - `git rev-list <rev> subdir` treats `subdir` as a revision argument and errors (`object not found: subdir`) instead of interpreting it as a path limiter.
- Deprecation advice missing:
  - `git show HEAD` does not emit warning about deprecated graft files (expected text includes `git replace`).

### Plan for fix
- Add graft map loading + parent override in rev-list commit graph traversal.
- Add trailing-argument rev/path disambiguation in `grit rev-list` command parser for existing filesystem paths when no `--` separator is provided.
- Add graft deprecation advice message in `grit show`, gated by `advice.graftFileDeprecated`.
