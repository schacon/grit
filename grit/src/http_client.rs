//! Shared HTTP(S) client for smart HTTP transport: `http.proxy`, `GIT_ASKPASS`, and `GIT_TRACE_CURL`.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::net::UnixStream;

use anyhow::{bail, Context, Result};
use base64::Engine;
use flate2::write::GzEncoder;
use flate2::Compression;
use grit_lib::config::{parse_bool, parse_i64, ConfigSet};
use url::Url;

/// Pre-built ureq agent or SOCKS-over-Unix tunnel for `http.proxy`.
#[derive(Clone)]
pub struct HttpClientContext {
    transport: Transport,
    trace_curl: Option<TraceCurl>,
    proxy_raw: Option<String>,
    git_protocol_header: Option<String>,
    post_buffer: usize,
    credential_use_http_path: bool,
    credential_username: Option<String>,
    cookie_header: Option<String>,
    smart_http_enabled: bool,
    auth_cache: Arc<Mutex<Option<AuthCredentials>>>,
}

#[derive(Clone)]
enum Transport {
    Ureq(ureq::Agent),
    /// RFC 7230 absolute-form requests through an HTTP proxy (`GET http://host/...`).
    HttpForward {
        proxy_host: String,
        proxy_port: u16,
        proxy_basic: Option<String>,
    },
    SocksUnix {
        socket_path: PathBuf,
    },
}

#[derive(Clone)]
struct TraceCurl {
    path: TraceCurlDest,
    components: String,
    redact: bool,
}

#[derive(Clone)]
enum TraceCurlDest {
    Stderr,
    File(String),
}

/// Validate `http.proxy` from `git clone -c http.proxy=...` before clap runs, so invalid URLs
/// fail with Git-shaped stderr even when other arguments confuse the parser (t5564).
pub fn validate_clone_proxy_from_argv(rest: &[String]) -> Result<()> {
    if let Some(v) = last_command_line_config_value(rest, "http.proxy") {
        validate_proxy_url(&v)?;
    }
    Ok(())
}

fn last_command_line_config_value(rest: &[String], want_key: &str) -> Option<String> {
    let mut out = None;
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "-c" && i + 1 < rest.len() {
            let entry = &rest[i + 1];
            if let Some((k, v)) = entry.split_once('=') {
                if k.trim() == want_key {
                    out = Some(v.trim().to_string());
                }
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

fn parse_protocol_version(value: &str) -> Option<u8> {
    match value.trim() {
        "0" => Some(0),
        "1" => Some(1),
        "2" => Some(2),
        _ => None,
    }
}

fn resolve_git_protocol_header(config: &ConfigSet) -> Option<String> {
    let version = config
        .get("protocol.version")
        .as_deref()
        .and_then(parse_protocol_version)
        .unwrap_or_else(crate::protocol_wire::effective_client_protocol_version);
    match version {
        0 => None,
        1 => Some("version=1".to_string()),
        _ => Some("version=2".to_string()),
    }
}

impl HttpClientContext {
    /// Build transport from merged Git config (`http.proxy`, etc.).
    pub fn from_config_set(config: &ConfigSet) -> Result<Self> {
        let trace_curl = trace_curl_from_env();
        let proxy_raw = config.get("http.proxy");
        let transport = build_transport(config)?;
        let git_protocol_header = resolve_git_protocol_header(config);
        let post_buffer = config
            .get("http.postBuffer")
            .as_deref()
            .and_then(|v| parse_i64(v).ok())
            .filter(|v| *v > 0)
            .map_or(1024 * 1024, |v| usize::try_from(v).unwrap_or(1024 * 1024));
        let credential_use_http_path = config
            .get("credential.useHttpPath")
            .as_deref()
            .map(|v| parse_bool(v).unwrap_or(false))
            .unwrap_or(false);
        let credential_username = config
            .get("credential.username")
            .filter(|s| !s.trim().is_empty());
        let cookie_header = build_cookie_header(config)?;
        let smart_http_enabled = std::env::var("GIT_SMART_HTTP")
            .ok()
            .is_none_or(|v| v.trim() != "0");
        Ok(Self {
            transport,
            trace_curl,
            proxy_raw,
            git_protocol_header,
            post_buffer,
            credential_use_http_path,
            credential_username,
            cookie_header,
            smart_http_enabled,
            auth_cache: Arc::new(Mutex::new(None)),
        })
    }

    /// Default agent (no proxy, trace from environment only).
    pub fn default_agent() -> Result<Self> {
        Self::from_config_set(&ConfigSet::new())
    }

    /// Whether smart HTTP should be used for discovery/rpc.
    #[must_use]
    pub fn smart_http_enabled(&self) -> bool {
        self.smart_http_enabled
    }

    /// Return the configured `Git-Protocol` request header value for this context.
    ///
    /// Returns `None` when protocol v0 is selected and the header should be suppressed.
    #[must_use]
    pub fn git_protocol_header(&self) -> Option<&str> {
        self.git_protocol_header.as_deref()
    }

    /// Perform GET, returning the response body. Fails on HTTP status >= 400.
    pub fn get(&self, url: &str) -> Result<Vec<u8>> {
        self.get_with_git_protocol(url, self.git_protocol_header.as_deref())
    }

    /// Perform GET with an explicit `Git-Protocol` header override.
    ///
    /// Passing `None` suppresses the header entirely for the request.
    pub fn get_with_git_protocol(
        &self,
        url: &str,
        git_protocol_header: Option<&str>,
    ) -> Result<Vec<u8>> {
        self.trace_proxy_auth_header();
        self.trace_request_start("GET", url, self.smart_http_enabled);
        if let Some(v) = git_protocol_header {
            self.trace_outgoing_header(&format!("Git-Protocol: {v}"));
        }
        self.trace_cookie_header();
        let cached_auth = self.cached_authorization_header();
        let first = self.http_get_once(url, cached_auth.as_deref(), git_protocol_header)?;
        self.trace_response_status(first.status, &first.reason);
        if first.status != 401 {
            if first.status >= 400 {
                return Err(http_access_error(url, first.status));
            }
            return Ok(first.body);
        }
        let auth_challenges = first.www_authenticate_challenges();

        let mut auth = self
            .credentials_from_fill(url, &auth_challenges)?
            .unwrap_or(self.default_auth_for_url(url)?);
        if auth.username.is_empty() {
            auth.username = self.askpass_username(url)?;
        }
        if auth.password.is_empty() {
            auth.password = self.askpass_password(url, &auth.username)?;
        }

        self.trace_auth_header();
        let retry =
            self.http_get_once(url, Some(&auth.authorization_header()), git_protocol_header)?;
        let mut credential_input = self.credential_input_for_url(url)?;
        credential_input.insert("username".to_string(), auth.username.clone());
        credential_input.insert("password".to_string(), auth.password.clone());
        let reject_extras = credential_challenge_extras(&auth_challenges);
        self.trace_response_status(retry.status, &retry.reason);
        if retry.status >= 400 {
            let _ = self.run_credential_action("reject", &credential_input, &reject_extras);
            self.clear_cached_auth();
            return Err(http_access_error(url, retry.status));
        }
        let _ = self.run_credential_action("approve", &credential_input, &[]);
        self.store_cached_auth(auth);
        Ok(retry.body)
    }

    /// Perform POST with given headers, returning the body.
    pub fn post(
        &self,
        url: &str,
        content_type: &str,
        accept: &str,
        body: &[u8],
    ) -> Result<Vec<u8>> {
        self.post_with_git_protocol(
            url,
            content_type,
            accept,
            body,
            self.git_protocol_header.as_deref(),
        )
    }

    /// Perform POST with an explicit `Git-Protocol` header override.
    ///
    /// Passing `None` suppresses the header entirely for the request.
    pub fn post_with_git_protocol(
        &self,
        url: &str,
        content_type: &str,
        accept: &str,
        body: &[u8],
        git_protocol_header: Option<&str>,
    ) -> Result<Vec<u8>> {
        self.trace_proxy_auth_header();
        self.trace_request_start("POST", url, self.smart_http_enabled);
        self.trace_outgoing_header(&format!("Content-Type: {content_type}"));
        self.trace_outgoing_header(&format!("Accept: {accept}"));
        if let Some(v) = git_protocol_header {
            self.trace_outgoing_header(&format!("Git-Protocol: {v}"));
        }
        self.trace_cookie_header();
        let (payload, gzip_enabled) = self.encode_post_payload(body)?;
        if gzip_enabled {
            self.trace_outgoing_header("Content-Encoding: gzip");
        }

        let chunked = payload.len() > self.post_buffer;
        if chunked {
            self.trace_outgoing_header("Transfer-Encoding: chunked");
        } else {
            self.trace_outgoing_header(&format!("Content-Length: {}", payload.len()));
        }

        let cached_auth = self.cached_authorization_header();
        let first = self.http_post_once(
            url,
            content_type,
            accept,
            &payload,
            cached_auth.as_deref(),
            gzip_enabled,
            chunked,
            git_protocol_header,
        )?;
        self.trace_response_status(first.status, &first.reason);
        if first.status != 401 {
            if first.status >= 400 {
                return Err(http_access_error(url, first.status));
            }
            return Ok(first.body);
        }
        let auth_challenges = first.www_authenticate_challenges();

        let mut auth = self
            .credentials_from_fill(url, &auth_challenges)?
            .unwrap_or(self.default_auth_for_url(url)?);
        if auth.username.is_empty() {
            auth.username = self.askpass_username(url)?;
        }
        if auth.password.is_empty() {
            auth.password = self.askpass_password(url, &auth.username)?;
        }
        self.trace_auth_header();

        let retry = self.http_post_once(
            url,
            content_type,
            accept,
            &payload,
            Some(&auth.authorization_header()),
            gzip_enabled,
            chunked,
            git_protocol_header,
        )?;
        let mut credential_input = self.credential_input_for_url(url)?;
        credential_input.insert("username".to_string(), auth.username.clone());
        credential_input.insert("password".to_string(), auth.password.clone());
        let reject_extras = credential_challenge_extras(&auth_challenges);
        self.trace_response_status(retry.status, &retry.reason);
        if retry.status >= 400 {
            let _ = self.run_credential_action("reject", &credential_input, &reject_extras);
            self.clear_cached_auth();
            return Err(http_access_error(url, retry.status));
        }
        let _ = self.run_credential_action("approve", &credential_input, &[]);
        self.store_cached_auth(auth);
        Ok(retry.body)
    }

    fn http_get_once(
        &self,
        url: &str,
        auth_header: Option<&str>,
        git_protocol_header: Option<&str>,
    ) -> Result<RawHttpResponse> {
        let request_url = discovery_url_for_mode(url, self.smart_http_enabled);
        match &self.transport {
            Transport::Ureq(agent) => {
                let mut req = agent
                    .get(&request_url)
                    .set("User-Agent", &crate::http_smart::agent_header());
                if let Some(v) = git_protocol_header {
                    req = req.set("Git-Protocol", v);
                }
                if let Some(cookie) = self.cookie_header.as_deref() {
                    req = req.set("Cookie", cookie);
                }
                if let Some(v) = auth_header {
                    req = req.set("Authorization", v);
                }
                match req.call() {
                    Ok(resp) => {
                        let status = resp.status();
                        let reason = resp.status_text().to_string();
                        let headers = response_headers(&resp);
                        let mut body = Vec::new();
                        resp.into_reader()
                            .read_to_end(&mut body)
                            .context("read GET body")?;
                        Ok(RawHttpResponse {
                            status,
                            reason,
                            headers,
                            body,
                        })
                    }
                    Err(ureq::Error::Status(code, resp)) => {
                        let reason = resp.status_text().to_string();
                        let headers = response_headers(&resp);
                        let mut body = Vec::new();
                        resp.into_reader()
                            .read_to_end(&mut body)
                            .context("read GET error body")?;
                        Ok(RawHttpResponse {
                            status: code,
                            reason,
                            headers,
                            body,
                        })
                    }
                    Err(err) => Err(anyhow::anyhow!("GET {url}: {err}")),
                }
            }
            Transport::HttpForward {
                proxy_host,
                proxy_port,
                proxy_basic,
            } => {
                let req = build_proxy_get_request(
                    &request_url,
                    proxy_basic.as_deref(),
                    auth_header,
                    git_protocol_header,
                    self.cookie_header.as_deref(),
                    self.smart_http_enabled,
                )?;
                http_over_tcp_forward(proxy_host, *proxy_port, &req)
            }
            Transport::SocksUnix { socket_path } => {
                let req = build_get_request(
                    &request_url,
                    auth_header,
                    git_protocol_header,
                    self.cookie_header.as_deref(),
                    self.smart_http_enabled,
                )?;
                http_over_socks_unix(socket_path, &request_url, &req)
            }
        }
    }

    fn http_post_once(
        &self,
        url: &str,
        content_type: &str,
        accept: &str,
        body: &[u8],
        auth_header: Option<&str>,
        gzip_enabled: bool,
        chunked: bool,
        git_protocol_header: Option<&str>,
    ) -> Result<RawHttpResponse> {
        let request_url = discovery_url_for_mode(url, self.smart_http_enabled);
        match &self.transport {
            Transport::Ureq(agent) => {
                let mut req = agent
                    .post(&request_url)
                    .set("Content-Type", content_type)
                    .set("Accept", accept)
                    .set("User-Agent", &crate::http_smart::agent_header());
                if let Some(v) = git_protocol_header {
                    req = req.set("Git-Protocol", v);
                }
                if gzip_enabled {
                    req = req.set("Content-Encoding", "gzip");
                }
                if let Some(cookie) = self.cookie_header.as_deref() {
                    req = req.set("Cookie", cookie);
                }
                if let Some(v) = auth_header {
                    req = req.set("Authorization", v);
                }
                let send_result = if chunked {
                    let mut cur = std::io::Cursor::new(body);
                    req.send(&mut cur)
                } else {
                    req.send_bytes(body)
                };
                match send_result {
                    Ok(resp) => {
                        let status = resp.status();
                        let reason = resp.status_text().to_string();
                        let headers = response_headers(&resp);
                        let mut out = Vec::new();
                        resp.into_reader()
                            .read_to_end(&mut out)
                            .context("read POST body")?;
                        Ok(RawHttpResponse {
                            status,
                            reason,
                            headers,
                            body: out,
                        })
                    }
                    Err(ureq::Error::Status(code, resp)) => {
                        let reason = resp.status_text().to_string();
                        let headers = response_headers(&resp);
                        let mut out = Vec::new();
                        resp.into_reader()
                            .read_to_end(&mut out)
                            .context("read POST error body")?;
                        Ok(RawHttpResponse {
                            status: code,
                            reason,
                            headers,
                            body: out,
                        })
                    }
                    Err(err) => Err(anyhow::anyhow!("POST {url}: {err}")),
                }
            }
            Transport::HttpForward {
                proxy_host,
                proxy_port,
                proxy_basic,
            } => {
                let req = build_proxy_post_request(
                    &request_url,
                    content_type,
                    accept,
                    body,
                    proxy_basic.as_deref(),
                    auth_header,
                    gzip_enabled,
                    chunked,
                    git_protocol_header,
                    self.cookie_header.as_deref(),
                    self.smart_http_enabled,
                )?;
                http_over_tcp_forward(proxy_host, *proxy_port, &req)
            }
            Transport::SocksUnix { socket_path } => {
                let req = build_post_request(
                    &request_url,
                    content_type,
                    accept,
                    body,
                    auth_header,
                    gzip_enabled,
                    chunked,
                    git_protocol_header,
                    self.cookie_header.as_deref(),
                    self.smart_http_enabled,
                )?;
                http_over_socks_unix(socket_path, &request_url, &req)
            }
        }
    }

    fn cached_authorization_header(&self) -> Option<String> {
        let guard = self
            .auth_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.as_ref().map(AuthCredentials::authorization_header)
    }

    fn store_cached_auth(&self, auth: AuthCredentials) {
        let mut guard = self
            .auth_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(auth);
    }

    fn clear_cached_auth(&self) {
        let mut guard = self
            .auth_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = None;
    }

    fn encode_post_payload(&self, body: &[u8]) -> Result<(Vec<u8>, bool)> {
        if body.len() <= 1024 {
            return Ok((body.to_vec(), false));
        }
        let mut gz = GzEncoder::new(Vec::new(), Compression::best());
        gz.write_all(body).context("gzip request body")?;
        let payload = gz.finish().context("finalize gzip body")?;
        Ok((payload, true))
    }

    fn trace_auth_header(&self) {
        let Some(ref t) = self.trace_curl else {
            return;
        };
        if !trace_component_enabled(&t.components, "http") {
            return;
        }
        if t.redact {
            t.write_line("=> Send header: Authorization: Basic <redacted>\n");
        } else {
            t.write_line("=> Send header: Authorization: Basic <not-redacted>\n");
        }
    }

    fn credential_input_for_url(&self, url: &str) -> Result<BTreeMap<String, String>> {
        let parsed = Url::parse(url).with_context(|| format!("bad URL {url}"))?;
        let mut input = BTreeMap::new();
        // `url=` matches Git and lets helpers (e.g. git-credential-osxkeychain) keychain lookup
        // the same way as `git credential fill`.
        let mut cred_lookup_url = parsed.clone();
        let _ = cred_lookup_url.set_password(None);
        input.insert("url".to_string(), cred_lookup_url.to_string());
        input.insert("protocol".to_string(), parsed.scheme().to_string());
        let host = host_header_value(&parsed);
        input.insert("host".to_string(), host);
        if self.credential_use_http_path {
            let path = parsed.path().trim_start_matches('/');
            if !path.is_empty() {
                input.insert("path".to_string(), path.to_string());
            }
        }
        if let Some(user) = self
            .credential_username
            .as_deref()
            .filter(|u| !u.is_empty())
        {
            input.insert("username".to_string(), user.to_string());
        } else if !parsed.username().is_empty() {
            input.insert("username".to_string(), parsed.username().to_string());
        }
        Ok(input)
    }

    fn run_credential_action(
        &self,
        action: &str,
        input: &BTreeMap<String, String>,
        extras: &[(String, String)],
    ) -> Result<BTreeMap<String, String>> {
        let exe = std::env::current_exe().context("resolve current executable for credential")?;
        let mut child = Command::new(exe)
            .arg("credential")
            .arg(action)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("spawn credential {action}"))?;
        {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("credential {action}: missing stdin"))?;
            for (k, v) in extras {
                writeln!(stdin, "{k}={v}")?;
            }
            for (k, v) in input {
                writeln!(stdin, "{k}={v}")?;
            }
            writeln!(stdin)?;
        }
        let out = child
            .wait_with_output()
            .with_context(|| format!("wait credential {action}"))?;
        if !out.status.success() {
            bail!("credential {action} exited with status {}", out.status);
        }
        let mut map = BTreeMap::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if line.trim().is_empty() {
                break;
            }
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.to_string(), v.to_string());
            }
        }
        Ok(map)
    }

    fn credentials_from_fill(
        &self,
        url: &str,
        auth_challenges: &[String],
    ) -> Result<Option<AuthCredentials>> {
        let input = self.credential_input_for_url(url)?;
        let extras = credential_fill_extras(auth_challenges);
        let filled = self.run_credential_action("fill", &input, &extras)?;
        let username = filled
            .get("username")
            .cloned()
            .or_else(|| input.get("username").cloned())
            .unwrap_or_default();
        let password = filled.get("password").cloned().unwrap_or_default();
        if username.is_empty() && password.is_empty() {
            return Ok(None);
        }
        Ok(Some(AuthCredentials { username, password }))
    }

    fn default_auth_for_url(&self, url: &str) -> Result<AuthCredentials> {
        let input = self.credential_input_for_url(url)?;
        let username = input.get("username").cloned().unwrap_or_default();
        Ok(AuthCredentials {
            username,
            password: String::new(),
        })
    }

    fn askpass_username(&self, url: &str) -> Result<String> {
        let prompt = format!("Username for '{}': ", credential_prompt_origin(url)?);
        run_askpass(&prompt)
    }

    fn askpass_password(&self, url: &str, username: &str) -> Result<String> {
        let encoded_user: String =
            url::form_urlencoded::byte_serialize(username.as_bytes()).collect();
        let prompt = format!(
            "Password for '{}://{}@{}': ",
            credential_prompt_scheme(url)?,
            encoded_user,
            credential_prompt_host(url)?
        );
        run_askpass(&prompt)
    }

    fn trace_request_start(&self, method: &str, url: &str, smart_http_enabled: bool) {
        let Some(ref t) = self.trace_curl else {
            return;
        };
        if !trace_component_enabled(&t.components, "http") {
            return;
        }
        let shown_url = discovery_url_for_mode(url, smart_http_enabled);
        t.write_line(&format!("=> Send header: {method} {shown_url} HTTP/1.1\n"));
    }

    fn trace_response_status(&self, status: u16, text: &str) {
        let Some(ref t) = self.trace_curl else {
            return;
        };
        if !trace_component_enabled(&t.components, "http") {
            return;
        }
        t.write_line(&format!("<= Recv header: HTTP/1.1 {status} {text}\n"));
    }

    fn trace_outgoing_header(&self, line: &str) {
        let Some(ref t) = self.trace_curl else {
            return;
        };
        if !trace_component_enabled(&t.components, "http") {
            return;
        }
        t.write_line(&format!("=> Send header: {line}\n"));
    }

    fn trace_cookie_header(&self) {
        let Some(cookie) = self.cookie_header.as_deref() else {
            return;
        };
        let Some(ref t) = self.trace_curl else {
            return;
        };
        if !trace_component_enabled(&t.components, "http") {
            return;
        }
        if t.redact {
            let redacted = redact_cookie_header(cookie);
            t.write_line(&format!("=> Send header: Cookie: {redacted}\n"));
        } else {
            t.write_line(&format!("=> Send header: Cookie: {cookie}\n"));
        }
    }

    fn trace_proxy_auth_header(&self) {
        let Some(ref t) = self.trace_curl else {
            return;
        };
        if !trace_component_enabled(&t.components, "http") {
            return;
        }
        let Some(ref raw) = self.proxy_raw else {
            return;
        };
        let with_scheme = if raw.contains("://") {
            raw.clone()
        } else {
            format!("http://{raw}")
        };
        let Ok(parsed) = Url::parse(&with_scheme) else {
            return;
        };
        if parsed.scheme().to_ascii_lowercase().starts_with("socks") {
            return;
        }
        if parsed.username().is_empty() {
            return;
        }
        let line = if t.redact {
            "Proxy-Authorization: Basic <redacted>".to_string()
        } else if let Some(pass) = parsed.password() {
            let cred = format!("{}:{}", parsed.username(), pass);
            format!(
                "Proxy-Authorization: Basic {}",
                base64::engine::general_purpose::STANDARD.encode(cred.as_bytes())
            )
        } else {
            "Proxy-Authorization: Basic <redacted>".to_string()
        };
        t.write_line(&format!("=> Send header: {line}\n"));
    }
}

impl TraceCurl {
    fn write_line(&self, line: &str) {
        match &self.path {
            TraceCurlDest::Stderr => {
                let mut l = std::io::stderr().lock();
                let _ = l.write_all(line.as_bytes());
                let _ = l.flush();
            }
            TraceCurlDest::File(p) => {
                if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
                    let _ = f.write_all(line.as_bytes());
                    let _ = f.flush();
                    let _ = f.sync_all();
                }
            }
        }
    }
}

fn response_headers(resp: &ureq::Response) -> Vec<(String, String)> {
    resp.headers_names()
        .into_iter()
        .filter_map(|name| {
            resp.header(&name)
                .map(|value| (name.to_ascii_lowercase(), value.to_string()))
        })
        .collect()
}

struct RawHttpResponse {
    status: u16,
    reason: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl RawHttpResponse {
    fn www_authenticate_challenges(&self) -> Vec<String> {
        self.headers
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case("www-authenticate"))
            .map(|(_, value)| value.clone())
            .collect()
    }
}

#[derive(Clone)]
struct AuthCredentials {
    username: String,
    password: String,
}

impl AuthCredentials {
    fn authorization_header(&self) -> String {
        let cred = format!("{}:{}", self.username, self.password);
        let encoded = base64::engine::general_purpose::STANDARD.encode(cred.as_bytes());
        format!("Basic {encoded}")
    }
}

fn http_over_tcp_forward(host: &str, port: u16, req: &[u8]) -> Result<RawHttpResponse> {
    let mut sock = TcpStream::connect((host, port))
        .with_context(|| format!("connect to proxy {host}:{port}"))?;
    let _ = sock.set_read_timeout(Some(Duration::from_secs(120)));
    let _ = sock.set_write_timeout(Some(Duration::from_secs(120)));
    sock.write_all(req).context("write to proxy")?;
    sock.flush()?;
    read_http_response(&mut sock)
}

fn build_proxy_get_request(
    target_url: &str,
    proxy_basic: Option<&str>,
    auth_header: Option<&str>,
    git_protocol_header: Option<&str>,
    cookie_header: Option<&str>,
    smart_http_enabled: bool,
) -> Result<Vec<u8>> {
    let parsed = Url::parse(target_url).with_context(|| format!("bad URL {target_url}"))?;
    let host = host_header_value(&parsed);
    let request_url = discovery_url_for_mode(target_url, smart_http_enabled);
    let mut s = format!(
        "GET {request_url} HTTP/1.1\r\n\
         Host: {host}\r\n\
         User-Agent: {}\r\n\
         Connection: close\r\n\
         Accept: */*\r\n",
        crate::http_smart::agent_header()
    );
    if let Some(v) = git_protocol_header {
        s.push_str(&format!("Git-Protocol: {v}\r\n"));
    }
    if let Some(b) = proxy_basic {
        s.push_str(&format!("Proxy-Authorization: Basic {b}\r\n"));
    }
    if let Some(a) = auth_header {
        s.push_str(&format!("Authorization: {a}\r\n"));
    }
    if let Some(cookie) = cookie_header {
        s.push_str(&format!("Cookie: {cookie}\r\n"));
    }
    s.push_str("\r\n");
    Ok(s.into_bytes())
}

fn build_proxy_post_request(
    target_url: &str,
    content_type: &str,
    accept: &str,
    body: &[u8],
    proxy_basic: Option<&str>,
    auth_header: Option<&str>,
    gzip_enabled: bool,
    chunked: bool,
    git_protocol_header: Option<&str>,
    cookie_header: Option<&str>,
    smart_http_enabled: bool,
) -> Result<Vec<u8>> {
    let parsed = Url::parse(target_url).with_context(|| format!("bad URL {target_url}"))?;
    let host = host_header_value(&parsed);
    let request_url = discovery_url_for_mode(target_url, smart_http_enabled);
    let mut head = format!(
        "POST {request_url} HTTP/1.1\r\n\
         Host: {host}\r\n\
         Content-Type: {content_type}\r\n\
         Accept: {accept}\r\n\
         User-Agent: {}\r\n\
         Connection: close\r\n",
        crate::http_smart::agent_header()
    );
    if let Some(v) = git_protocol_header {
        head.push_str(&format!("Git-Protocol: {v}\r\n"));
    }
    if let Some(b) = proxy_basic {
        head.push_str(&format!("Proxy-Authorization: Basic {b}\r\n"));
    }
    if let Some(a) = auth_header {
        head.push_str(&format!("Authorization: {a}\r\n"));
    }
    if let Some(cookie) = cookie_header {
        head.push_str(&format!("Cookie: {cookie}\r\n"));
    }
    if gzip_enabled {
        head.push_str("Content-Encoding: gzip\r\n");
    }
    if chunked {
        head.push_str("Transfer-Encoding: chunked\r\n");
    } else {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    let mut out = head.into_bytes();
    if chunked {
        append_chunked_body(&mut out, body);
    } else {
        out.extend_from_slice(body);
    }
    Ok(out)
}

fn build_get_request(
    url: &str,
    auth_header: Option<&str>,
    git_protocol_header: Option<&str>,
    cookie_header: Option<&str>,
    smart_http_enabled: bool,
) -> Result<Vec<u8>> {
    let parsed = Url::parse(url).with_context(|| format!("bad URL {url}"))?;
    let path_q = url_path_and_query(&parsed);
    let host = host_header_value(&parsed);
    let request_path_q = discovery_url_for_mode(&path_q, smart_http_enabled);
    let mut s = format!(
        "GET {request_path_q} HTTP/1.1\r\n\
         Host: {host}\r\n\
         User-Agent: {}\r\n\
         Connection: close\r\n\
         Accept: */*\r\n\
         \r\n",
        crate::http_smart::agent_header()
    );
    if let Some(v) = git_protocol_header {
        let marker = "\r\n\r\n";
        if let Some(pos) = s.find(marker) {
            s.insert_str(pos, &format!("Git-Protocol: {v}\r\n"));
        }
    }
    if let Some(a) = auth_header {
        let marker = "\r\n\r\n";
        if let Some(pos) = s.find(marker) {
            s.insert_str(pos, &format!("Authorization: {a}\r\n"));
        }
    }
    if let Some(cookie) = cookie_header {
        let marker = "\r\n\r\n";
        if let Some(pos) = s.find(marker) {
            s.insert_str(pos, &format!("Cookie: {cookie}\r\n"));
        }
    }
    Ok(s.into_bytes())
}

fn build_post_request(
    url: &str,
    content_type: &str,
    accept: &str,
    body: &[u8],
    auth_header: Option<&str>,
    gzip_enabled: bool,
    chunked: bool,
    git_protocol_header: Option<&str>,
    cookie_header: Option<&str>,
    smart_http_enabled: bool,
) -> Result<Vec<u8>> {
    let parsed = Url::parse(url).with_context(|| format!("bad URL {url}"))?;
    let path_q = url_path_and_query(&parsed);
    let host = host_header_value(&parsed);
    let request_path_q = discovery_url_for_mode(&path_q, smart_http_enabled);
    let mut head = format!(
        "POST {request_path_q} HTTP/1.1\r\n\
         Host: {host}\r\n\
         Content-Type: {content_type}\r\n\
         Accept: {accept}\r\n\
         User-Agent: {}\r\n\
         Connection: close\r\n\
         \r\n",
        crate::http_smart::agent_header()
    );
    if let Some(v) = git_protocol_header {
        let marker = "\r\n\r\n";
        if let Some(pos) = head.find(marker) {
            head.insert_str(pos, &format!("Git-Protocol: {v}\r\n"));
        }
    }
    if let Some(a) = auth_header {
        let marker = "\r\n\r\n";
        if let Some(pos) = head.find(marker) {
            head.insert_str(pos, &format!("Authorization: {a}\r\n"));
        }
    }
    if let Some(cookie) = cookie_header {
        let marker = "\r\n\r\n";
        if let Some(pos) = head.find(marker) {
            head.insert_str(pos, &format!("Cookie: {cookie}\r\n"));
        }
    }
    if gzip_enabled {
        let marker = "\r\n\r\n";
        if let Some(pos) = head.find(marker) {
            head.insert_str(pos, "Content-Encoding: gzip\r\n");
        }
    }
    if chunked {
        let marker = "\r\n\r\n";
        if let Some(pos) = head.find(marker) {
            head.insert_str(pos, "Transfer-Encoding: chunked\r\n");
        }
    } else {
        let marker = "\r\n\r\n";
        if let Some(pos) = head.find(marker) {
            head.insert_str(pos, &format!("Content-Length: {}\r\n", body.len()));
        }
    }
    let mut out = head.into_bytes();
    if chunked {
        append_chunked_body(&mut out, body);
    } else {
        out.extend_from_slice(body);
    }
    Ok(out)
}

fn append_chunked_body(out: &mut Vec<u8>, body: &[u8]) {
    if body.is_empty() {
        out.extend_from_slice(b"0\r\n\r\n");
        return;
    }
    const CHUNK: usize = 16 * 1024;
    let mut offset = 0usize;
    while offset < body.len() {
        let end = std::cmp::min(offset + CHUNK, body.len());
        let size = end - offset;
        out.extend_from_slice(format!("{size:x}\r\n").as_bytes());
        out.extend_from_slice(&body[offset..end]);
        out.extend_from_slice(b"\r\n");
        offset = end;
    }
    out.extend_from_slice(b"0\r\n\r\n");
}

fn credential_fill_extras(auth_challenges: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(2 + auth_challenges.len());
    out.push(("capability[]".to_string(), "authtype".to_string()));
    out.push(("capability[]".to_string(), "state".to_string()));
    out.extend(credential_challenge_extras(auth_challenges));
    out
}

fn credential_challenge_extras(auth_challenges: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(auth_challenges.len());
    for challenge in auth_challenges {
        out.push(("wwwauth[]".to_string(), challenge.clone()));
    }
    out
}

fn url_path_and_query(url: &Url) -> String {
    let mut p = url.path().to_string();
    if p.is_empty() {
        p.push('/');
    }
    if let Some(q) = url.query() {
        p.push('?');
        p.push_str(q);
    }
    p
}

fn host_header_value(url: &Url) -> String {
    let host = url.host_str().unwrap_or("localhost");
    match url.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_string(),
    }
}

fn resolve_target_ipv4(url: &Url) -> Result<std::net::Ipv4Addr> {
    let host = url.host_str().context("URL has no host")?;
    let port = url.port_or_known_default().unwrap_or(80);
    let addr = format!("{host}:{port}")
        .to_socket_addrs()
        .with_context(|| format!("resolve {host}"))?
        .find(|a| matches!(a, std::net::SocketAddr::V4(_)))
        .context("no IPv4 address for host (SOCKS4 requires IPv4)")?;
    match addr {
        std::net::SocketAddr::V4(v4) => Ok(*v4.ip()),
        _ => bail!("expected IPv4"),
    }
}

#[cfg(unix)]
fn http_over_socks_unix(
    socket_path: &Path,
    target_url: &str,
    http_bytes: &[u8],
) -> Result<RawHttpResponse> {
    let url = Url::parse(target_url).with_context(|| format!("bad URL {target_url}"))?;
    let ip = resolve_target_ipv4(&url)?;
    let port = url
        .port_or_known_default()
        .context("URL missing port for SOCKS target")?;

    let mut sock = UnixStream::connect(socket_path)
        .with_context(|| format!("connect SOCKS unix socket {}", socket_path.display()))?;
    let _ = sock.set_read_timeout(Some(Duration::from_secs(120)));
    let _ = sock.set_write_timeout(Some(Duration::from_secs(120)));

    let mut req = Vec::with_capacity(9 + 1);
    req.push(4u8);
    req.push(1);
    req.extend_from_slice(&port.to_be_bytes());
    req.extend_from_slice(&ip.octets());
    req.push(0);

    sock.write_all(&req).context("SOCKS4 request")?;
    let mut reply = [0u8; 8];
    sock.read_exact(&mut reply).context("SOCKS4 reply")?;
    if reply[1] != 0x5a {
        bail!("SOCKS4 connection failed (reply {})", reply[1]);
    }

    trace_socks_granted_after_handshake();

    sock.write_all(http_bytes).context("write HTTP request")?;
    sock.flush()?;

    read_http_response(&mut sock)
}

#[cfg(not(unix))]
fn http_over_socks_unix(
    _socket_path: &Path,
    _target_url: &str,
    _http_bytes: &[u8],
) -> Result<RawHttpResponse> {
    bail!("SOCKS proxy over Unix socket is not supported on this platform")
}

fn read_http_response(r: &mut impl Read) -> Result<RawHttpResponse> {
    let mut reader = BufReader::new(r);
    let mut status_line = String::new();
    reader.read_line(&mut status_line).context("read status")?;
    let status_line = status_line.trim_end_matches(['\r', '\n']);
    let mut parts = status_line.split_whitespace();
    let _http = parts.next();
    let status: u16 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .context("bad HTTP status line")?;
    let reason = parts.collect::<Vec<_>>().join(" ");

    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).context("read header")?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some((_, value)) = headers.last_mut() {
                if !value.is_empty() {
                    value.push(' ');
                }
                value.push_str(line.trim());
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }

    let mut body = Vec::new();
    if let Some(cl) = headers.iter().find(|(k, _)| k == "content-length") {
        let len: usize = cl.1.parse().context("content-length")?;
        body.resize(len, 0);
        reader.read_exact(&mut body).context("read body")?;
    } else if headers
        .iter()
        .any(|(k, v)| k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked"))
    {
        loop {
            let mut size_line = String::new();
            reader.read_line(&mut size_line).context("chunk size")?;
            let size_line = size_line.trim_end_matches(['\r', '\n']);
            let chunk_len = usize::from_str_radix(size_line.trim(), 16)
                .map_err(|_| anyhow::anyhow!("bad chunk size"))?;
            if chunk_len == 0 {
                let mut crlf = [0u8; 2];
                let _ = reader.read_exact(&mut crlf);
                break;
            }
            let mut chunk = vec![0u8; chunk_len];
            reader.read_exact(&mut chunk).context("chunk data")?;
            body.extend_from_slice(&chunk);
            let mut crlf = [0u8; 2];
            reader.read_exact(&mut crlf).context("chunk crlf")?;
        }
    } else {
        reader
            .read_to_end(&mut body)
            .context("read body until EOF")?;
    }

    Ok(RawHttpResponse {
        status,
        reason,
        headers,
        body,
    })
}

fn trace_socks_granted_after_handshake() {
    let Some(t) = trace_curl_from_env() else {
        return;
    };
    t.write_line("== Info: SOCKS4 request granted\n");
}

fn read_response_body(mut reader: impl Read, context: &'static str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    reader.read_to_end(&mut out).context(context)?;
    Ok(out)
}

fn build_cookie_header(config: &ConfigSet) -> Result<Option<String>> {
    let Some(path_raw) = config.get("http.cookieFile") else {
        return Ok(None);
    };
    let path_raw = path_raw.trim();
    if path_raw.is_empty() {
        return Ok(None);
    }
    let lines = read_cookie_file_lines(path_raw)?;
    if lines.is_empty() {
        return Ok(None);
    }
    let mut parts = Vec::new();
    for line in lines {
        let v = line.trim();
        if v.is_empty() {
            continue;
        }
        if let Some(stripped) = strip_cookie_value(v) {
            if !stripped.is_empty() {
                parts.push(stripped);
            }
        }
    }
    if parts.is_empty() {
        return Ok(None);
    }
    Ok(Some(parts.join("; ")))
}

fn read_cookie_file_lines(path_raw: &str) -> Result<Vec<String>> {
    let path = PathBuf::from(path_raw);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read_to_string(&path)
        .with_context(|| format!("read cookie file '{}'", path.display()))?;
    let mut out = Vec::new();
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        out.push(trimmed.to_string());
    }
    Ok(out)
}

fn strip_cookie_value(line: &str) -> Option<String> {
    if let Some(v) = line.strip_prefix("Set-Cookie:") {
        return Some(v.trim().to_string());
    }
    if let Some(v) = line.strip_prefix("set-cookie:") {
        return Some(v.trim().to_string());
    }
    Some(line.trim().to_string())
}

fn redact_cookie_header(cookie: &str) -> String {
    let mut out = Vec::new();
    for part in cookie.split(';') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        if let Some((k, _)) = p.split_once('=') {
            out.push(format!("{}=<redacted>", k.trim()));
        } else {
            out.push(p.to_string());
        }
    }
    out.join("; ")
}

fn discovery_url_for_mode(url: &str, smart_http_enabled: bool) -> String {
    if smart_http_enabled {
        return url.to_string();
    }
    if let Some((prefix, _)) = url.split_once("/info/refs?service=") {
        return format!("{prefix}/info/refs");
    }
    if let Some(prefix) = url.strip_suffix("/git-upload-pack") {
        return format!("{prefix}/info/refs");
    }
    if let Some(prefix) = url.strip_suffix("/git-receive-pack") {
        return format!("{prefix}/info/refs");
    }
    url.to_string()
}

fn trace_component_enabled(components: &str, want: &str) -> bool {
    let c = components.trim();
    if c.is_empty() {
        return true;
    }
    c.split(|ch: char| ch == ':' || ch == ',' || ch.is_whitespace())
        .any(|p| p.eq_ignore_ascii_case(want))
}

fn trace_curl_from_env() -> Option<TraceCurl> {
    let Ok(raw) = std::env::var("GIT_TRACE_CURL") else {
        return None;
    };
    let raw = raw.trim();
    if raw.is_empty() || raw == "0" || raw.eq_ignore_ascii_case("false") {
        return None;
    }
    let path = if raw == "1" || raw.eq_ignore_ascii_case("true") {
        TraceCurlDest::Stderr
    } else {
        TraceCurlDest::File(raw.to_string())
    };
    let components = std::env::var("GIT_TRACE_CURL_COMPONENTS").unwrap_or_default();
    let redact = std::env::var("GIT_TRACE_REDACT").ok().as_deref() != Some("0");
    Some(TraceCurl {
        path,
        components,
        redact,
    })
}

fn http_access_error(url: &str, status: u16) -> anyhow::Error {
    anyhow::anyhow!("unable to access '{url}': The requested URL returned error: {status}")
}

fn build_transport(config: &ConfigSet) -> Result<Transport> {
    let Some(raw_proxy) = config.get("http.proxy") else {
        return Ok(Transport::Ureq(ureq::Agent::new()));
    };
    let raw_proxy = raw_proxy.trim();
    if raw_proxy.is_empty() {
        return Ok(Transport::Ureq(ureq::Agent::new()));
    }
    validate_proxy_url(raw_proxy)?;
    let with_scheme = if raw_proxy.contains("://") {
        raw_proxy.to_string()
    } else {
        format!("http://{raw_proxy}")
    };
    let parsed =
        Url::parse(&with_scheme).map_err(|_| anyhow::anyhow!("Invalid proxy URL '{raw_proxy}'"))?;

    if let Some(path) = socks_unix_proxy_socket(raw_proxy, &parsed) {
        return Ok(Transport::SocksUnix { socket_path: path });
    }

    let scheme = parsed.scheme().to_ascii_lowercase();
    if scheme == "http" {
        let mut p = parsed.clone();
        fill_proxy_password_via_askpass(&mut p)?;
        let proxy_host = p
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid proxy URL '{raw_proxy}'"))?
            .to_string();
        let proxy_port = p.port_or_known_default().unwrap_or(80);
        let proxy_basic = proxy_basic_token(&p)?;
        return Ok(Transport::HttpForward {
            proxy_host,
            proxy_port,
            proxy_basic,
        });
    }

    let proxy_url = normalize_proxy_url_for_ureq(raw_proxy, &parsed)?;
    let proxy =
        ureq::Proxy::new(&proxy_url).with_context(|| format!("invalid proxy URL '{raw_proxy}'"))?;
    Ok(Transport::Ureq(
        ureq::AgentBuilder::new().proxy(proxy).build(),
    ))
}

fn proxy_basic_token(url: &Url) -> Result<Option<String>> {
    if url.username().is_empty() {
        return Ok(None);
    }
    let pass = url.password().unwrap_or("");
    let cred = format!("{}:{}", url.username(), pass);
    Ok(Some(
        base64::engine::general_purpose::STANDARD.encode(cred.as_bytes()),
    ))
}

/// `socks*://localhost/abs/path.sock` style proxy (Git uses a path after localhost).
///
/// Important: `url::Url::path()` applies percent-decoding, which breaks double-encoded
/// test paths like `%2530.sock` → must decode exactly once from the raw string (t5564).
fn socks_unix_proxy_socket(raw_proxy: &str, url: &Url) -> Option<PathBuf> {
    let scheme = url.scheme().to_ascii_lowercase();
    if !scheme.starts_with("socks") {
        return None;
    }
    let host = url.host_str()?;
    if !host.eq_ignore_ascii_case("localhost") {
        return None;
    }
    let lower = raw_proxy.to_ascii_lowercase();
    let key = "localhost";
    let idx = lower.find(key)?;
    let after_host = &raw_proxy[idx + key.len()..];
    if after_host.starts_with(':') {
        return None;
    }
    if !after_host.starts_with('/') {
        return None;
    }
    if after_host.len() <= 1 {
        return None;
    }
    Some(PathBuf::from(percent_decode_path(after_host)))
}

fn percent_decode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let a = bytes[i + 1];
            let b = bytes[i + 2];
            if let (Some(h1), Some(h2)) = (from_hex(a), from_hex(b)) {
                out.push(char::from(h1 * 16 + h2));
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Git-style checks from `http.c` (paths only for SOCKS; host must be localhost).
fn validate_proxy_url(raw: &str) -> Result<()> {
    let with_scheme = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("http://{raw}")
    };
    let parsed =
        Url::parse(&with_scheme).map_err(|_| anyhow::anyhow!("Invalid proxy URL '{raw}'"))?;
    let path = parsed.path();
    let has_extra_path = path.len() > 1;
    if has_extra_path {
        let scheme = parsed.scheme().to_ascii_lowercase();
        if !scheme.starts_with("socks") {
            bail!("Invalid proxy URL '{raw}': only SOCKS proxies support paths");
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid proxy URL '{raw}'"))?;
        if !host.eq_ignore_ascii_case("localhost") {
            bail!("Invalid proxy URL '{raw}': host must be localhost if a path is present");
        }
    }
    Ok(())
}

fn normalize_proxy_url_for_ureq(raw: &str, parsed: &Url) -> Result<String> {
    if socks_unix_proxy_socket(raw, parsed).is_some() {
        bail!("internal: SOCKS unix proxy should not use ureq");
    }
    let mut url = parsed.clone();
    fill_proxy_password_via_askpass(&mut url)?;
    Ok(url.to_string())
}

fn fill_proxy_password_via_askpass(url: &mut Url) -> Result<()> {
    if url.password().is_some() {
        return Ok(());
    }
    let user = url.username();
    if user.is_empty() {
        return Ok(());
    }
    let askpass = match std::env::var("GIT_ASKPASS") {
        Ok(p) if !p.trim().is_empty() => p,
        _ => return Ok(()),
    };
    let display = {
        let mut u = url.clone();
        let _ = u.set_password(None);
        let mut s = u.to_string();
        // Match Git/credential helper prompts: no trailing slash for host:port-only URLs (t5564).
        if u.path() == "/" || u.path().is_empty() {
            while s.ends_with('/') {
                s.pop();
            }
        }
        s
    };
    let prompt = format!("Password for '{display}': ");
    let out = Command::new(&askpass)
        .arg(&prompt)
        .output()
        .with_context(|| format!("run GIT_ASKPASS ({askpass})"))?;
    if !out.status.success() {
        bail!("failed to get proxy password from askpass");
    }
    let pass = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if pass.is_empty() {
        bail!("askpass returned an empty proxy password");
    }
    url.set_password(Some(&pass))
        .map_err(|_| anyhow::anyhow!("could not set proxy password in URL"))?;
    Ok(())
}

fn run_askpass(prompt: &str) -> Result<String> {
    let askpass = std::env::var("GIT_ASKPASS").unwrap_or_default();
    if askpass.trim().is_empty() {
        bail!("failed to get credentials: GIT_ASKPASS is not set");
    }
    let out = Command::new(&askpass)
        .arg(prompt)
        .output()
        .with_context(|| format!("run GIT_ASKPASS ({askpass})"))?;
    if !out.status.success() {
        bail!("askpass failed");
    }
    let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if value.is_empty() {
        bail!("askpass returned empty value");
    }
    Ok(value)
}

fn credential_prompt_origin(url: &str) -> Result<String> {
    let parsed = Url::parse(url).with_context(|| format!("bad URL {url}"))?;
    let scheme = parsed.scheme();
    let host = host_header_value(&parsed);
    Ok(format!("{scheme}://{host}"))
}

fn credential_prompt_scheme(url: &str) -> Result<String> {
    let parsed = Url::parse(url).with_context(|| format!("bad URL {url}"))?;
    Ok(parsed.scheme().to_string())
}

fn credential_prompt_host(url: &str) -> Result<String> {
    let parsed = Url::parse(url).with_context(|| format!("bad URL {url}"))?;
    Ok(host_header_value(&parsed))
}
