# t8490-ls-remote-extra — help exit code

## Failure

Harness reported 31/32 passing. Local repro: test 32 `ls-remote --help shows usage` failed under POSIX `sh`.

## Cause

`grit ls-remote --help` exited **129** (Git-style usage). In POSIX `sh`, exit statuses >125 are treated as command failure for `set -e` / `&&`, so `grit ls-remote --help >out 2>&1 && grep -i usage out` failed even though `grep` succeeded.

## Fix

- `parse_cmd_args`: lone `--help` with upstream synopsis → exit **0**; lone `-h` → **129** (unchanged for t0450).
- Clap `DisplayHelp` / `DisplayHelpOnMissingArgumentOrSubcommand`: exit **0** when argv contains `--help`, else **129**.
- Renamed behavior documented on `print_upstream_synopsis_and_exit` (now takes explicit exit code).

## Verification

- `./scripts/run-tests.sh t8490-ls-remote-extra.sh` → 32/32
- `grit ls-remote -h` still exits 129
