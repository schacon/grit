# t3012-ls-files-dedup

- Fixed `grit ls-files` to match upstream `git/builtin/ls-files.c` for `--deduplicate` with `-t`/`-s`/`-u`, `-d`/`-m` on unmerged paths, and `-t` tags (`C` vs `M`) during merge conflicts.
- Ran `./scripts/run-tests.sh t3012-ls-files-dedup.sh` → 3/3 pass; refreshed `data/test-files.csv` and dashboards.
