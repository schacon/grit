# t3406-rebase-message

- **Issue:** Two failures: `rebase -m` wrote "First, rewinding..." to stdout (test captures stdout only); glued `-Cnot-a-number` was not split so clap errored before `validate_compat_syntax`.
- **Fix:** `preprocess_rebase_argv` splits any non-empty `-C<suffix>` into two args; moved rewind notices from `println!` to `eprintln!` in fast-forward and replay paths (matches Git: progress on stderr).
