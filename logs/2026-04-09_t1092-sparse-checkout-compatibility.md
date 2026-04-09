# t1092 sparse-checkout compatibility

- Harness reported 104/106: two `test_expect_failure` cases passed with grit (TODO vanished).
- Flipped to `test_expect_success`: grep recurse-submodules without full-index expansion; `diff --check` with pathspec under sparse directory.
- Re-ran `./scripts/run-tests.sh t1092-sparse-checkout-compatibility.sh` → 106/106.
