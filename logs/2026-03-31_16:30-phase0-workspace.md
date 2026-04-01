# Phase 0: Workspace and Shared Infrastructure

**Started:** 2026-03-31 16:30  
**Task items:** 0.1, 0.2, 0.3, 0.4, 0.5, 0.6

## 0.1 Cargo workspace

Creating:
- Workspace `Cargo.toml` at root
- `gust/` binary crate (thin CLI layer)
- `gust-lib/` library crate (all engine logic)

Lints: deny `clippy::unwrap_used`, `clippy::expect_used` in production code.

## Status

- [~] 0.1 workspace structure
- [ ] 0.2 CLI dispatch
- [ ] 0.3 repository discovery
- [ ] 0.4 OID / SHA-1
- [ ] 0.5 loose object store
- [ ] 0.6 test harness
