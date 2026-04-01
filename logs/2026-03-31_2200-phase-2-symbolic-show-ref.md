# Phase 2.1/2.2/2.3: symbolic-ref + show-ref (2026-03-31)

- Implemented `grit symbolic-ref` with read/update/delete flows, `--short`, `--no-recurse`, `-q`, `-d`, and target validation rules.
- Implemented `grit show-ref` with pattern listing, `--branches`/`--tags`, `--verify`, `--exists`, `-d/--dereference`, `-s/--hash`, and quiet/exit-code behavior used by ported tests.
- Ported upstream tests into local harness:
  - `tests/t1401-symbolic-ref.sh`
  - `tests/t1403-show-ref.sh`
  - `tests/t1422-show-ref-exists.sh`
  - shared helper `tests/show-ref-exists-tests.sh`
- Added new scripts to `tests/harness/selected-tests.txt`.
- Validation run:
  - `cargo fmt` PASS
  - `cargo clippy --workspace --all-targets -- -D warnings` PASS
  - `cargo test --workspace` PASS
  - `tests/t1401-symbolic-ref.sh` PASS
  - `tests/t1403-show-ref.sh` PASS
  - `tests/t1422-show-ref-exists.sh` PASS
