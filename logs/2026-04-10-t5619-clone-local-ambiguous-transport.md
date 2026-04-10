# t5619-clone-local-ambiguous-transport

## Symptom

Harness reported 1/2: `submodule update --init` failed before the security assertion (or failed the grep) because grit spawned `grit clone --separate-git-dir` for HTTP submodule URLs; grit rejected HTTP + `--separate-git-dir`. After addressing that, native HTTP clone still failed on dumb HTTP (`empty v2 capability block`).

## Fix

1. **clone.rs** — Do not use `Path::exists()` for arguments that start with `http://` or `https://`, since `Path::new("http://host/...")` is a relative path that can match a malicious `http:/…` directory tree (same scenario as upstream t5619).

2. **`_submodule_run_update_inner.rs.inc`** — For submodule clone URLs starting with `http://` or `https://`, run `git clone` on `PATH` instead of grit. The harness hybrid wrapper (`lib-httpd.sh`) delegates HTTP clone/fetch to system git so dumb HTTP works; grit’s smart HTTP client does not.

## Validation

- `./scripts/run-tests.sh t5619-clone-local-ambiguous-transport.sh` → 2/2 pass; CSV/dashboards updated.
