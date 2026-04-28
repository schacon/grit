#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

/// Exercise smart-HTTP ref discovery against an opt-in CORS-enabled server.
///
/// Compile/run with:
///
/// ```text
/// GRIT_WASM_TEST_REMOTE_URL=http://127.0.0.1:<port>/smart/repo.git \
///   wasm-pack test --chrome --headless grit-wasm
/// ```
///
/// The server should be `test-httpd --cors` or equivalent and must support
/// protocol-v2 upload-pack discovery plus `ls-refs`.
#[wasm_bindgen_test(async)]
async fn ls_refs_against_cors_smart_http_remote() {
    let Some(remote_url) = option_env!("GRIT_WASM_TEST_REMOTE_URL") else {
        // Opt-in integration test: normal wasm test compiles should not require
        // a live Git HTTP server.
        return;
    };
    let refs = grit_wasm::remote::ls_refs(remote_url)
        .await
        .expect("ls_refs against CORS smart HTTP remote");

    assert!(
        refs.length() > 0,
        "remote should advertise at least one ref"
    );
}
