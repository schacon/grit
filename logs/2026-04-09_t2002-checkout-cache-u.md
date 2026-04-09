# t2002-checkout-cache-u

- Fast-forwarded branch to `origin/main` (da46a675).
- `./scripts/run-tests.sh t2002-checkout-cache-u.sh`: **3/3** passing.
- No Rust changes required: `grit checkout-index -u` already updates cached stat fields after checkout (`checkout_index.rs`: `args.update_stat` → `refresh_stat_for_entry`).
