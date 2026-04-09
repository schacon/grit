# t9901-git-web--browse

- Added `grit web--browse` (`grit/src/commands/web_browse.rs`): resolves browser from `--browser` / `-t` / `--tool`, `-c` / `web.browser`, or PATH probing like Git’s script; `browser.<tool>.path` for executables with spaces; `browser.<tool>.cmd` via `sh -c 'eval "$BROWSER_CMD \"\$@\""'` with URL args (matches Git’s subshell).
- Registered in `KNOWN_COMMANDS` and `dispatch` in `main.rs`, exported module in `commands/mod.rs`.
- `./scripts/run-tests.sh t9901-git-web--browse.sh` → 5/5; dashboards CSV/HTML updated.
