# t12630-rev-parse-is-bare

## Issue

Harness failed at "bare: HEAD not valid without commits": `grit rev-parse HEAD` printed `HEAD` and exited 0 in an empty bare repo.

## Fix

Removed the `rev_parse` special case that echoed literal `HEAD` when bare and the symref target had no OID. Resolution now fails like `git rev-parse --verify HEAD`, matching t12630 and t13330 (orphan branch).

## Verification

- `./scripts/run-tests.sh t12630-rev-parse-is-bare.sh` → 33/33
- `cargo test -p grit-lib --lib` → pass
