//! Browser HTTP transport helpers for smart Git requests.

use base64::prelude::*;
use grit_lib::error::{Error, Result};
use url::Url;

/// Browser-ready request URL and headers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserRequestParts {
    /// URL with userinfo stripped so browser `fetch` accepts it.
    pub url: String,
    /// Headers to send with the request.
    pub headers: Vec<(String, String)>,
}

/// Convert URL-embedded credentials into an `Authorization` header.
///
/// Browsers reject `fetch("https://user:pass@example.com/...")`; this helper
/// strips userinfo from the URL and returns `Authorization: Basic ...` instead.
///
/// # Errors
///
/// Returns an error when `raw_url` is not a valid URL.
pub fn browser_request_parts(
    raw_url: &str,
    extra_headers: &[(String, String)],
) -> Result<BrowserRequestParts> {
    let mut url = Url::parse(raw_url).map_err(|err| Error::Message(err.to_string()))?;
    let username = url.username().to_string();
    let password = url.password().map(ToOwned::to_owned);
    let mut headers = extra_headers.to_vec();

    if !username.is_empty() || password.is_some() {
        let password = password.unwrap_or_default();
        let encoded = BASE64_STANDARD.encode(format!("{username}:{password}"));
        headers.push(("Authorization".to_string(), format!("Basic {encoded}")));
        url.set_username("")
            .map_err(|_| Error::Message("failed to strip username from URL".to_string()))?;
        url.set_password(None)
            .map_err(|_| Error::Message("failed to strip password from URL".to_string()))?;
    }

    Ok(BrowserRequestParts {
        url: url.to_string(),
        headers,
    })
}

/// Fetch bytes from the browser using `window.fetch`.
#[cfg(target_arch = "wasm32")]
pub async fn fetch_bytes(
    method: &str,
    raw_url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
) -> std::result::Result<Vec<u8>, wasm_bindgen::JsValue> {
    use js_sys::Uint8Array;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Headers, Request, RequestInit, RequestMode, Response};

    let parts = browser_request_parts(raw_url, headers)
        .map_err(|err| wasm_bindgen::JsValue::from_str(&err.to_string()))?;
    let request_headers = Headers::new()?;
    for (name, value) in &parts.headers {
        request_headers.append(name, value)?;
    }

    let init = RequestInit::new();
    init.set_method(method);
    init.set_mode(RequestMode::Cors);
    init.set_headers(&request_headers);
    let body_value;
    if let Some(body) = body {
        body_value = Uint8Array::from(body).into();
        init.set_body(&body_value);
    }

    let request = Request::new_with_str_and_init(&parts.url, &init)?;
    let window =
        web_sys::window().ok_or_else(|| wasm_bindgen::JsValue::from_str("missing window"))?;
    let response_value = JsFuture::from(window.fetch_with_request(&request)).await?;
    let response: Response = response_value.dyn_into()?;
    if !response.ok() {
        return Err(wasm_bindgen::JsValue::from_str(&format!(
            "HTTP {} {}",
            response.status(),
            response.status_text()
        )));
    }
    let buffer = JsFuture::from(response.array_buffer()?).await?;
    Ok(Uint8Array::new(&buffer).to_vec())
}

/// Fetch smart-Git bytes with standard content negotiation headers.
#[cfg(target_arch = "wasm32")]
pub async fn fetch_git_rpc(
    raw_url: &str,
    service: &str,
    body: &[u8],
    git_protocol: Option<&str>,
) -> std::result::Result<Vec<u8>, wasm_bindgen::JsValue> {
    let mut headers = vec![
        (
            "Content-Type".to_string(),
            format!("application/x-{service}-request"),
        ),
        (
            "Accept".to_string(),
            format!("application/x-{service}-result"),
        ),
    ];
    if let Some(protocol) = git_protocol {
        headers.push(("Git-Protocol".to_string(), protocol.to_string()));
    }
    fetch_bytes("POST", raw_url, &headers, Some(body)).await
}

/// Fetch smart-Git discovery bytes with optional `Git-Protocol` negotiation.
#[cfg(target_arch = "wasm32")]
pub async fn fetch_git_discovery(
    raw_url: &str,
    git_protocol: Option<&str>,
) -> std::result::Result<Vec<u8>, wasm_bindgen::JsValue> {
    let mut headers = Vec::new();
    if let Some(protocol) = git_protocol {
        headers.push(("Git-Protocol".to_string(), protocol.to_string()));
    }
    fetch_bytes("GET", raw_url, &headers, None).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_userinfo_into_basic_auth_header() {
        let parts = browser_request_parts(
            "https://user:token@example.com/repo.git",
            &[("Git-Protocol".to_string(), "version=2".to_string())],
        )
        .unwrap();

        assert_eq!(parts.url, "https://example.com/repo.git");
        assert!(parts
            .headers
            .iter()
            .any(|(name, value)| name == "Authorization" && value == "Basic dXNlcjp0b2tlbg=="));
        assert!(parts
            .headers
            .iter()
            .any(|(name, value)| name == "Git-Protocol" && value == "version=2"));
    }

    #[test]
    fn leaves_credentialless_urls_unchanged() {
        let parts = browser_request_parts("https://example.com/repo.git", &[]).unwrap();

        assert_eq!(parts.url, "https://example.com/repo.git");
        assert!(parts.headers.is_empty());
    }
}
