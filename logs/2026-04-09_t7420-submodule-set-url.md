# t7420-submodule-set-url

- Confirmed `./scripts/run-tests.sh t7420-submodule-set-url.sh` passes (3/3).
- Aligned `submodule set-url` with Git: after updating `.gitmodules`, update superproject `submodule.<name>.url` using `resolve_submodule_super_url` (same as `submodule sync`), only when the submodule URL exists in `.git/config`; remove stray `submodule.<path>.url` when logical name differs from path so `test_cmp_config submodule.thepath.url` sees no duplicate key.
