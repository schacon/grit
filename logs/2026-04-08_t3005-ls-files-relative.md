# t3005-ls-files-relative

- Fixed `ls-files` cwd-relative output: `path_to_slash` dropped `ParentDir` (`..`) components, collapsing paths like `../../never-mind-me` to `never-mind-me`.
- Extended `cwd_prefix_bytes` / display path logic so lexical cwd can pair with a canonically spelled work tree (symlink / different spelling).
- `--error-unmatch`: separate index vs others matching (`-o` does not treat tracked-only matches as success); report every failing pathspec plus the `git add` advice; use `ExplicitExit` to avoid double `error:` prefix and wrong exit code.

Harness: `./scripts/run-tests.sh t3005-ls-files-relative.sh` → 4/4 pass.
