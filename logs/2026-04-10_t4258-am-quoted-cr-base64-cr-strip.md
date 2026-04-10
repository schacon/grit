## t4258-am-quoted-cr — base64 payload CRLF

**Issue:** Test 2 (`am warn if quoted-cr is found`) failed: `test_must_fail git am` did not fail because the patch applied even though default `--quoted-cr` is `warn` (should keep quoted CRLF in the diff so apply fails, while still printing `quoted CRLF detected`).

**Cause:** After `decode_transfer_payload` left `\r` in place for `QuotedCrAction::Warn`, `parse_mbox_with_opts` split the decoded body on `\n` and unconditionally stripped trailing `\r` when `!keep_cr`, normalizing CRLF to LF and making the patch apply.

**Fix:** For `Content-Transfer-Encoding: base64`, skip that post-split strip so line endings match Git’s mailinfo behavior for quoted CRLF.

**Validation:** `./scripts/run-tests.sh t4258-am-quoted-cr.sh` => 4/4; `cargo check -p grit-rs`; `cargo test -p grit-lib --lib`.
