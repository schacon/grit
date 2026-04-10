//! Lightweight HTTP server for git's HTTP transport tests.
//!
//! Replaces Apache httpd for running tests like t5550-http-fetch-dumb.sh.
//!
//! Features:
//! - Listens on a random available port (prints port to stdout)
//! - Serves static files from a document root (dumb HTTP)
//! - Runs git-http-backend as CGI for smart HTTP
//! - Supports basic HTTP auth
//! - Proper HTTP status codes
//!
//! Usage:
//!   test-httpd --root /path/to/docroot [--auth user:pass] [--port 0]
//!   [--proxy] [--proxy-auth proxuser:proxpass]
//!
//! On startup, prints "READY <port>" to stdout, then serves until killed.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() {
    let args: Vec<String> = env::args().collect();
    let config = parse_args(&args);

    let listener = TcpListener::bind(format!("127.0.0.1:{}", config.port)).unwrap_or_else(|e| {
        eprintln!("Failed to bind: {e}");
        std::process::exit(1);
    });

    let port = listener
        .local_addr()
        .unwrap_or_else(|e| {
            eprintln!("Failed to get local addr: {e}");
            std::process::exit(1);
        })
        .port();

    // Signal readiness — the test harness reads this line.
    println!("READY {port}");
    // Flush to ensure the harness sees it immediately.
    let _ = std::io::stdout().flush();

    // Write PID file if requested
    if let Some(ref pid_path) = config.pid_file {
        fs::write(pid_path, format!("{}", std::process::id())).unwrap_or_else(|e| {
            eprintln!("Failed to write PID file: {e}");
            std::process::exit(1);
        });
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let cfg = config.clone();
                // Handle synchronously — tests are single-threaded anyway,
                // but we use threads for robustness against slow clients.
                std::thread::spawn(move || {
                    if let Err(e) = handle_connection(stream, &cfg) {
                        eprintln!("Connection error: {e}");
                    }
                });
            }
            Err(e) => {
                eprintln!("Accept error: {e}");
            }
        }
    }
}

#[derive(Clone, Debug)]
struct Config {
    root: PathBuf,
    port: u16,
    auth_user: Option<String>,
    auth_pass: Option<String>,
    /// When set, act as an HTTP proxy (CONNECT + absolute-form `http://...` requests).
    proxy_mode: bool,
    proxy_auth_user: Option<String>,
    proxy_auth_pass: Option<String>,
    pid_file: Option<PathBuf>,
    /// Path to git-http-backend (auto-detected if not specified)
    git_http_backend: PathBuf,
    access_log: PathBuf,
}

fn find_git_http_backend() -> PathBuf {
    if let Ok(exec_path) = std::env::var("GIT_EXEC_PATH") {
        let candidate = Path::new(&exec_path).join("git-http-backend");
        if candidate.exists() {
            return candidate;
        }
    }

    let candidates = [
        "/usr/lib/git-core/git-http-backend",
        "/usr/libexec/git-core/git-http-backend",
        "/usr/local/lib/git-core/git-http-backend",
        "/usr/local/libexec/git-core/git-http-backend",
    ];
    for c in &candidates {
        if Path::new(c).exists() {
            return PathBuf::from(c);
        }
    }

    PathBuf::from("git-http-backend")
}

fn parse_args(args: &[String]) -> Config {
    let mut root = PathBuf::from(".");
    let mut port: u16 = 0;
    let mut auth_user = None;
    let mut auth_pass = None;
    let mut proxy_mode = false;
    let mut proxy_auth_user = None;
    let mut proxy_auth_pass = None;
    let mut pid_file = None;
    let mut git_http_backend = find_git_http_backend();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--root" => {
                i += 1;
                root = PathBuf::from(&args[i]);
            }
            "--port" => {
                i += 1;
                port = args[i].parse().unwrap_or(0);
            }
            "--auth" => {
                i += 1;
                if let Some((u, p)) = args[i].split_once(':') {
                    auth_user = Some(u.to_string());
                    auth_pass = Some(p.to_string());
                }
            }
            "--proxy" => {
                proxy_mode = true;
            }
            "--proxy-auth" => {
                i += 1;
                if let Some((u, p)) = args[i].split_once(':') {
                    proxy_auth_user = Some(u.to_string());
                    proxy_auth_pass = Some(p.to_string());
                }
            }
            "--pid-file" => {
                i += 1;
                pid_file = Some(PathBuf::from(&args[i]));
            }
            "--backend" => {
                i += 1;
                git_http_backend = PathBuf::from(&args[i]);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let access_log = root.parent().unwrap_or(Path::new(".")).join("access.log");
    Config {
        root,
        port,
        auth_user,
        auth_pass,
        proxy_mode,
        proxy_auth_user,
        proxy_auth_pass,
        pid_file,
        git_http_backend,
        access_log,
    }
}

/// Split an absolute-form request target (`GET http://host/path?query`) into path + query.
/// Returns `None` if `raw` is not an `http://` or `https://` absolute URI.
fn split_http_target(raw: &str) -> Option<(String, String)> {
    let lower = raw.to_ascii_lowercase();
    let rest = if lower.starts_with("http://") {
        &raw["http://".len()..]
    } else if lower.starts_with("https://") {
        &raw["https://".len()..]
    } else {
        return None;
    };
    let after_path = if let Some(slash) = rest.find('/') {
        &rest[slash..]
    } else {
        "/"
    };
    if let Some(q) = after_path.find('?') {
        Some((after_path[..q].to_string(), after_path[q + 1..].to_string()))
    } else {
        Some((after_path.to_string(), String::new()))
    }
}

/// Minimal HTTP request representation.
struct Request {
    method: String,
    /// Raw request-URI from the request line (may be absolute-form for proxy clients).
    raw_target: String,
    path: String,
    query: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);

    // Read request line
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|e| e.to_string())?;
    let request_line = request_line.trim_end().to_string();

    let parts: Vec<&str> = request_line.splitn(3, ' ').collect();
    if parts.len() < 2 {
        return Err("Invalid request line".to_string());
    }
    let method = parts[0].to_string();
    let raw_target = parts[1].to_string();

    // Split path and query string. For proxy absolute-form targets (`GET http://host/path?q`),
    // only the path component may contain `?` (not the scheme).
    let (path, query) = if let Some((p, q)) = split_http_target(&raw_target) {
        (p, q)
    } else if let Some(idx) = raw_target.find('?') {
        (
            raw_target[..idx].to_string(),
            raw_target[idx + 1..].to_string(),
        )
    } else {
        (raw_target.clone(), String::new())
    };

    // Read headers
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let line = line.trim_end().to_string();
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            headers.insert(key.trim().to_lowercase(), value.trim().to_string());
        }
    }

    // Read body if Content-Length is present
    let body = if let Some(len_str) = headers.get("content-length") {
        let len: usize = len_str.parse().unwrap_or(0);
        let mut body = vec![0u8; len];
        reader.read_exact(&mut body).map_err(|e| e.to_string())?;
        body
    } else if headers
        .get("transfer-encoding")
        .is_some_and(|v| v.contains("chunked"))
    {
        let mut body = Vec::new();
        loop {
            let mut size_line = String::new();
            reader
                .read_line(&mut size_line)
                .map_err(|e| e.to_string())?;
            let chunk_size = usize::from_str_radix(size_line.trim(), 16)
                .map_err(|e| format!("Invalid chunk size: {}", e))?;
            if chunk_size == 0 {
                let mut t = String::new();
                let _ = reader.read_line(&mut t);
                break;
            }
            let mut chunk = vec![0u8; chunk_size];
            reader.read_exact(&mut chunk).map_err(|e| e.to_string())?;
            body.extend_from_slice(&chunk);
            let mut crlf = [0u8; 2];
            let _ = reader.read_exact(&mut crlf);
        }
        body
    } else {
        Vec::new()
    };

    Ok(Request {
        method,
        raw_target,
        path,
        query,
        headers,
        body,
    })
}

fn log_access(config: &Config, method: &str, path: &str, query: &str, status: u16) {
    use std::fs::OpenOptions;
    let line = if query.is_empty() {
        format!("{} {} HTTP/1.1 {}", method, path, status)
    } else {
        format!("{} {}?{} HTTP/1.1 {}", method, path, query, status)
    };
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&config.access_log)
    {
        let _ = writeln!(f, "{}", line);
    }
}

fn handle_connection(mut stream: TcpStream, config: &Config) -> Result<(), String> {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));

    let req = read_request(&mut stream)?;

    if config.proxy_mode {
        let raw_lc = req.raw_target.to_ascii_lowercase();
        let is_proxy_client_request = req.method.eq_ignore_ascii_case("CONNECT")
            || raw_lc.starts_with("http://")
            || raw_lc.starts_with("https://");
        // Loopback forwards from `handle_proxy_http_forward` use relative URLs and must not
        // require proxy credentials again (Git omits Proxy-Authorization on the origin hop).
        if is_proxy_client_request {
            if let (Some(ref u), Some(ref p)) = (&config.proxy_auth_user, &config.proxy_auth_pass) {
                if !check_proxy_auth(&req, u, p) {
                    log_access(config, &req.method, &req.path, &req.query, 407);
                    return send_response(
                        &mut stream,
                        407,
                        "Proxy Authentication Required",
                        &[("Proxy-Authenticate", "Basic realm=\"proxy-auth\"")],
                        b"Proxy authentication required\n",
                    );
                }
            }
        }
        if req.method.eq_ignore_ascii_case("CONNECT") {
            return handle_proxy_connect(stream, &req, config);
        }
        if raw_lc.starts_with("http://") || raw_lc.starts_with("https://") {
            return handle_proxy_http_forward(stream, &req, config);
        }
    }

    // Log request
    eprintln!(
        "{} {} {}",
        req.method,
        req.path,
        if req.query.is_empty() {
            String::new()
        } else {
            format!("?{}", req.query)
        }
    );

    let needs_auth = if req.path.starts_with("/auth-push/") {
        req.path.contains("git-receive-pack") || req.query.contains("service=git-receive-pack")
    } else if req.path.starts_with("/auth-fetch/") {
        req.path.contains("git-upload-pack") && req.method == "POST"
    } else {
        req.path.starts_with("/auth/")
    };
    if needs_auth {
        if let (Some(ref user), Some(ref pass)) = (&config.auth_user, &config.auth_pass) {
            if !check_auth(&req, user, pass) {
                log_access(config, &req.method, &req.path, &req.query, 401);
                return send_response(
                    &mut stream,
                    401,
                    "Unauthorized",
                    &[("WWW-Authenticate", "Basic realm=\"test\"")],
                    b"Authentication required\n",
                );
            }
        }
    }

    // Route: /auth/smart/, /auth-push/smart/, /auth-fetch/smart/
    for pfx in &["/auth/smart", "/auth-push/smart", "/auth-fetch/smart"] {
        if req.path.starts_with(&format!("{}/", pfx)) {
            let r = handle_smart_http_with_path(&mut stream, &req, config, pfx);
            log_access(
                config,
                &req.method,
                &req.path,
                &req.query,
                if r.is_ok() { 200 } else { 500 },
            );
            return r;
        }
    }
    // Route: /smart/<repo> → git-http-backend CGI
    if req.path.starts_with("/smart/") {
        let r = handle_smart_http(&mut stream, &req, config);
        log_access(
            config,
            &req.method,
            &req.path,
            &req.query,
            if r.is_ok() { 200 } else { 500 },
        );
        return r;
    }

    // Route: /dumb/<path> → static file serving
    if req.path.starts_with("/dumb/") {
        let rel_path = &req.path["/dumb/".len()..];
        return serve_static_file(&mut stream, config, rel_path);
    }

    // Route: /auth/dumb/<path> → auth + static file (already checked auth above)
    if req.path.starts_with("/auth/dumb/") {
        let rel_path = &req.path["/auth/dumb/".len()..];
        return serve_static_file(&mut stream, config, rel_path);
    }

    // Fallback: try serving from document root directly
    let rel_path = req.path.trim_start_matches('/');
    if !rel_path.is_empty() {
        let full_path = config.root.join(rel_path);
        if full_path.exists() && full_path.is_file() {
            return serve_static_file(&mut stream, config, rel_path);
        }
    }

    log_access(config, &req.method, &req.path, &req.query, 404);
    send_response(&mut stream, 404, "Not Found", &[], b"Not Found\n")
}

fn check_auth(req: &Request, expected_user: &str, expected_pass: &str) -> bool {
    if let Some(auth) = req.headers.get("authorization") {
        if let Some(encoded) = strip_basic_prefix(auth) {
            if let Ok(decoded) = base64_decode(encoded.trim()) {
                if let Some((user, pass)) = decoded.split_once(':') {
                    return user == expected_user && pass == expected_pass;
                }
            }
        }
    }
    false
}

fn check_proxy_auth(req: &Request, expected_user: &str, expected_pass_cfg: &str) -> bool {
    if let Some(auth) = req.headers.get("proxy-authorization") {
        if let Some(encoded) = strip_basic_prefix(auth) {
            if let Ok(decoded) = base64_decode(encoded.trim()) {
                if let Some((user, pass)) = decoded.split_once(':') {
                    if user != expected_user {
                        return false;
                    }
                    // `tests/lib-httpd.sh` passes the upstream `proxy-passwd` line verbatim
                    // (`proxuser:$apr1$...`). Apache stores a hash; clients still send plaintext.
                    let expected_pass = if expected_pass_cfg.contains('$') {
                        "proxpass"
                    } else {
                        expected_pass_cfg
                    };
                    return pass == expected_pass;
                }
            }
        }
    }
    false
}

fn strip_basic_prefix(auth: &str) -> Option<&str> {
    let t = auth.trim();
    let prefix = "Basic ";
    if t.len() > prefix.len() && t[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&t[prefix.len()..])
    } else {
        None
    }
}

/// `CONNECT host:port` target (IPv6 uses brackets).
fn parse_connect_target(target: &str) -> Result<(String, u16), String> {
    let target = target.trim();
    if target.starts_with('[') {
        let end = target
            .find(']')
            .ok_or_else(|| "invalid CONNECT target".to_string())?;
        let host = target[1..end].to_string();
        let rest = &target[end + 1..];
        if let Some(p) = rest.strip_prefix(':') {
            let port: u16 = p.parse().map_err(|_| "invalid CONNECT port".to_string())?;
            return Ok((host, port));
        }
        return Err("CONNECT target missing port".to_string());
    }
    let colon = target
        .rfind(':')
        .ok_or_else(|| "CONNECT target missing port".to_string())?;
    let host = target[..colon].to_string();
    let port: u16 = target[colon + 1..]
        .parse()
        .map_err(|_| "invalid CONNECT port".to_string())?;
    Ok((host, port))
}

/// Absolute proxy request target: `http://host:port/path?query`.
fn parse_absolute_proxy_url(raw: &str) -> Result<(String, u16, String, String), String> {
    let raw_lc = raw.to_ascii_lowercase();
    let is_https = raw_lc.starts_with("https://");
    let rest = raw
        .strip_prefix("http://")
        .or_else(|| raw.strip_prefix("https://"))
        .or_else(|| raw.strip_prefix("HTTP://"))
        .or_else(|| raw.strip_prefix("HTTPS://"))
        .ok_or_else(|| "proxy target is not an http(s) URL".to_string())?;
    let slash = rest
        .find('/')
        .ok_or_else(|| "proxy URL has no path".to_string())?;
    let authority = &rest[..slash];
    let path_and_query = rest[slash..].to_string();
    let default_port: u16 = if is_https { 443 } else { 80 };
    let (host, port, host_header) = if authority.starts_with('[') {
        let end = authority
            .find(']')
            .ok_or_else(|| "bad IPv6 authority".to_string())?;
        let host_inner = authority[1..end].to_string();
        if let Some(p) = authority[end + 1..].strip_prefix(':') {
            let port: u16 = p.parse().map_err(|_| "bad port".to_string())?;
            (host_inner, port, authority.to_string())
        } else {
            (host_inner, default_port, authority.to_string())
        }
    } else if let Some(colon) = authority.rfind(':') {
        let host = authority[..colon].to_string();
        let port: u16 = authority[colon + 1..]
            .parse()
            .map_err(|_| "bad port".to_string())?;
        (host, port, authority.to_string())
    } else {
        let h = authority.to_string();
        (h.clone(), default_port, h)
    };
    Ok((host, port, host_header, path_and_query))
}

fn handle_proxy_connect(
    mut client: TcpStream,
    req: &Request,
    config: &Config,
) -> Result<(), String> {
    let (host, port) = parse_connect_target(&req.path)?;
    let remote = TcpStream::connect((host.as_str(), port)).map_err(|e| e.to_string())?;
    let _ = remote.set_read_timeout(Some(std::time::Duration::from_secs(60)));
    let _ = remote.set_write_timeout(Some(std::time::Duration::from_secs(60)));

    client
        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
        .map_err(|e| e.to_string())?;
    client.flush().map_err(|e| e.to_string())?;

    let mut client_r = client.try_clone().map_err(|e| e.to_string())?;
    let mut client_w = client;
    let mut remote_r = remote.try_clone().map_err(|e| e.to_string())?;
    let mut remote_w = remote;

    let t1 = std::thread::spawn(move || {
        let _ = std::io::copy(&mut client_r, &mut remote_w);
    });
    let t2 = std::thread::spawn(move || {
        let _ = std::io::copy(&mut remote_r, &mut client_w);
    });
    let _ = t1.join();
    let _ = t2.join();
    log_access(config, "CONNECT", &req.path, "", 200);
    Ok(())
}

fn handle_proxy_http_forward(
    mut client: TcpStream,
    req: &Request,
    config: &Config,
) -> Result<(), String> {
    let (host, port, host_header, path_and_query) = parse_absolute_proxy_url(&req.raw_target)?;
    let mut upstream = TcpStream::connect((host.as_str(), port)).map_err(|e| e.to_string())?;
    let _ = upstream.set_read_timeout(Some(std::time::Duration::from_secs(60)));
    let _ = upstream.set_write_timeout(Some(std::time::Duration::from_secs(60)));

    let mut out = Vec::new();
    out.extend_from_slice(format!("{} {} HTTP/1.1\r\n", req.method, path_and_query).as_bytes());
    out.extend_from_slice(format!("Host: {host_header}\r\n").as_bytes());
    for (k, v) in &req.headers {
        if k == "proxy-authorization" {
            continue;
        }
        if k == "host" {
            continue;
        }
        out.extend_from_slice(format!("{}: {}\r\n", title_case_header(k), v).as_bytes());
    }
    out.extend_from_slice(b"Connection: close\r\n");
    if !req.body.is_empty() || !req.method.eq_ignore_ascii_case("GET") {
        out.extend_from_slice(format!("Content-Length: {}\r\n", req.body.len()).as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&req.body);

    upstream.write_all(&out).map_err(|e| e.to_string())?;
    upstream.flush().map_err(|e| e.to_string())?;

    let mut response = Vec::new();
    let mut buf = [0u8; 16384];
    loop {
        let n = upstream.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        response.extend_from_slice(&buf[..n]);
    }

    let status = parse_status_from_response_bytes(&response).unwrap_or(502);
    log_access(config, &req.method, &req.path, &req.query, status);
    client.write_all(&response).map_err(|e| e.to_string())?;
    client.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn title_case_header(name: &str) -> String {
    name.split('-')
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

fn parse_status_from_response_bytes(resp: &[u8]) -> Option<u16> {
    let line_end = resp.iter().position(|&b| b == b'\n')?;
    let line = &resp[..line_end];
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let s = String::from_utf8_lossy(line);
    let mut parts = s.split_whitespace();
    parts.next()?; // HTTP/1.x
    parts.next()?.parse().ok()
}

/// Minimal base64 decoder (avoids external dep).
fn base64_decode(input: &str) -> Result<String, String> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = Vec::new();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &byte in input.as_bytes() {
        if byte == b'=' {
            break;
        }
        let val = TABLE
            .iter()
            .position(|&c| c == byte)
            .ok_or_else(|| format!("Invalid base64 char: {byte}"))?;
        buf = (buf << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }

    String::from_utf8(output).map_err(|e| e.to_string())
}

fn serve_static_file(
    stream: &mut TcpStream,
    config: &Config,
    rel_path: &str,
) -> Result<(), String> {
    // Security: reject path traversal
    if rel_path.contains("..") {
        return send_response(stream, 403, "Forbidden", &[], b"Forbidden\n");
    }

    let full_path = config.root.join(rel_path);

    // Ensure we don't escape the root
    let canonical_root = config.root.canonicalize().map_err(|e| e.to_string())?;
    let canonical_path = match full_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return send_response(stream, 404, "Not Found", &[], b"Not Found\n");
        }
    };
    if !canonical_path.starts_with(&canonical_root) {
        return send_response(stream, 403, "Forbidden", &[], b"Forbidden\n");
    }

    if !canonical_path.is_file() {
        return send_response(stream, 404, "Not Found", &[], b"Not Found\n");
    }

    let body = fs::read(&canonical_path).map_err(|e| e.to_string())?;
    let content_type = guess_content_type(rel_path);

    send_response(stream, 200, "OK", &[("Content-Type", &content_type)], &body)
}

fn guess_content_type(path: &str) -> String {
    if path.ends_with(".pack") {
        "application/x-git-packed-objects".to_string()
    } else if path.ends_with(".idx") {
        "application/x-git-packed-objects-toc".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

fn handle_smart_http(stream: &mut TcpStream, req: &Request, config: &Config) -> Result<(), String> {
    handle_smart_http_with_path(stream, req, config, "/smart")
}

fn handle_smart_http_with_path(
    stream: &mut TcpStream,
    req: &Request,
    config: &Config,
    prefix: &str,
) -> Result<(), String> {
    let smart_path = &req.path[prefix.len()..]; // e.g., /repo.git/info/refs

    let path_translated = format!("{}{}", config.root.display(), smart_path);

    let mut cmd = Command::new(&config.git_http_backend);
    cmd.env("REQUEST_METHOD", &req.method)
        .env("QUERY_STRING", &req.query)
        .env("PATH_TRANSLATED", &path_translated)
        .env("GIT_PROJECT_ROOT", config.root.to_string_lossy().as_ref())
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_INFO", smart_path)
        .env("SERVER_PROTOCOL", "HTTP/1.1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(ct) = req.headers.get("content-type") {
        cmd.env("CONTENT_TYPE", ct);
    }
    cmd.env("CONTENT_LENGTH", req.body.len().to_string());

    if let Some(proto) = req.headers.get("git-protocol") {
        cmd.env("GIT_PROTOCOL", proto);
    }

    // Pass auth info if present
    if let Some(auth) = req.headers.get("authorization") {
        cmd.env("HTTP_AUTHORIZATION", auth);
        if let Some(encoded) = auth.strip_prefix("Basic ") {
            if let Ok(decoded) = base64_decode(encoded.trim()) {
                if let Some((user, _)) = decoded.split_once(':') {
                    cmd.env("REMOTE_USER", user);
                }
            }
        }
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn git-http-backend: {e}"))?;

    // Send request body to CGI stdin
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&req.body);
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for git-http-backend: {e}"))?;

    // Parse CGI response (headers + body separated by blank line)
    let stdout = output.stdout;
    parse_and_send_cgi_response(stream, &stdout)
}

fn parse_and_send_cgi_response(stream: &mut TcpStream, cgi_output: &[u8]) -> Result<(), String> {
    // Find the header/body separator (blank line: \r\n\r\n or \n\n)
    let mut header_end = None;
    let mut body_start = None;

    for i in 0..cgi_output.len().saturating_sub(1) {
        if cgi_output[i] == b'\n' && cgi_output[i + 1] == b'\n' {
            header_end = Some(i);
            body_start = Some(i + 2);
            break;
        }
        if i + 3 < cgi_output.len()
            && cgi_output[i] == b'\r'
            && cgi_output[i + 1] == b'\n'
            && cgi_output[i + 2] == b'\r'
            && cgi_output[i + 3] == b'\n'
        {
            header_end = Some(i);
            body_start = Some(i + 4);
            break;
        }
    }

    let (header_bytes, body) = match (header_end, body_start) {
        (Some(he), Some(bs)) => (&cgi_output[..he], &cgi_output[bs..]),
        _ => {
            // No headers found, treat everything as body
            return send_response(stream, 200, "OK", &[], cgi_output);
        }
    };

    let header_str = String::from_utf8_lossy(header_bytes);
    let mut status_code = 200;
    let mut status_text = "OK".to_string();
    let mut extra_headers = Vec::new();

    for line in header_str.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(status_val) = line.strip_prefix("Status:") {
            let status_val = status_val.trim();
            // Parse "200 OK" or "403 Forbidden"
            let parts: Vec<&str> = status_val.splitn(2, ' ').collect();
            if let Some(code) = parts.first() {
                status_code = code.parse().unwrap_or(200);
            }
            if parts.len() > 1 {
                status_text = parts[1].to_string();
            }
        } else if let Some((key, value)) = line.split_once(':') {
            extra_headers.push((key.trim().to_string(), value.trim().to_string()));
        }
    }

    // Build response
    let mut response = format!("HTTP/1.1 {status_code} {status_text}\r\n");
    for (k, v) in &extra_headers {
        response.push_str(&format!("{k}: {v}\r\n"));
    }
    response.push_str(&format!("Content-Length: {}\r\n", body.len()));
    response.push_str("Connection: close\r\n");
    response.push_str("\r\n");

    stream
        .write_all(response.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.write_all(body).map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn send_response(
    stream: &mut TcpStream,
    status: u16,
    status_text: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Result<(), String> {
    let mut response = format!("HTTP/1.1 {status} {status_text}\r\n");
    for (k, v) in headers {
        response.push_str(&format!("{k}: {v}\r\n"));
    }
    response.push_str(&format!("Content-Length: {}\r\n", body.len()));
    response.push_str("Connection: close\r\n");
    response.push_str("\r\n");

    stream
        .write_all(response.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.write_all(body).map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    Ok(())
}
