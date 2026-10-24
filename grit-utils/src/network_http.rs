//! Local smart HTTP servers for network benchmarks.

use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::binary::resolve_http_server as resolve_server_bin;

/// `git http-backend` (git's own smart HTTP server) behind a small HTTP/1.1 listener, so
/// network benchmarks run both clients end to end against a stock git server.
pub struct GitHttpBackendServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    _git: PathBuf,
    _project_root: PathBuf,
}

impl GitHttpBackendServer {
    /// Serve every repository under `project_root` (`http://<addr>/<name>.git`), push included.
    pub fn start(git: &Path, project_root: PathBuf) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").context("bind git http-backend")?;
        let addr = listener.local_addr()?;
        let backend = git_http_backend_path(git)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let root = project_root.clone();
        // Blocking accept: no polling delay on any request. Drop wakes it with a connection.
        let thread = thread::spawn(move || {
            for stream in listener.incoming() {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(mut stream) = stream else {
                    continue;
                };
                let backend = backend.clone();
                let root = root.clone();
                thread::spawn(move || {
                    let _ = serve_backend_request(&backend, &root, &mut stream);
                });
            }
        });
        Ok(Self {
            addr,
            stop,
            thread: Some(thread),
            _git: git.to_path_buf(),
            _project_root: project_root,
        })
    }

    pub fn url(&self, repo_name: &str) -> String {
        format!("http://{}/{}", self.addr, repo_name)
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for GitHttpBackendServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(500));
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// Handle one HTTP request for `git http-backend` (used by the in-process server and tests).
pub fn handle_git_http_connection(
    git: &Path,
    project_root: &Path,
    stream: &mut TcpStream,
) -> Result<()> {
    serve_backend_request(&git_http_backend_path(git)?, project_root, stream)
}

/// A parsed request head.
struct RequestHead {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

impl RequestHead {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn serve_backend_request(
    backend: &Path,
    project_root: &Path,
    stream: &mut TcpStream,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(300)))?;
    stream.set_nodelay(true)?;
    let mut reader = BufReader::new(stream.try_clone().context("clone request stream")?);
    let Some(head) = read_request_head(&mut reader)? else {
        return Ok(());
    };
    if head
        .header("Expect")
        .is_some_and(|v| v.eq_ignore_ascii_case("100-continue"))
    {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
    }
    let chunked = head
        .header("Transfer-Encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    let body = if chunked {
        read_chunked_body(&mut reader)?
    } else {
        let len: usize = head
            .header("Content-Length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; len];
        reader.read_exact(&mut body).context("read HTTP body")?;
        body
    };

    let (path_info, query) = head
        .path
        .split_once('?')
        .unwrap_or((head.path.as_str(), ""));
    let mut cmd = Command::new(backend);
    configure_git_http_backend_cgi(&mut cmd, project_root, path_info, query, &head, body.len());
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().context("spawn git http-backend")?;
    // Feed the body from another thread so a large request and a large reply can't deadlock.
    let mut stdin = child.stdin.take().context("http-backend stdin")?;
    let writer = thread::spawn(move || {
        let _ = stdin.write_all(&body);
    });
    let mut stdout = BufReader::new(child.stdout.take().context("http-backend stdout")?);
    let (status, headers) = read_cgi_head(&mut stdout)?;

    let mut out = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in &headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    // No Content-Length from the CGI: the body runs to the end of the connection.
    out.push_str("Connection: close\r\n\r\n");
    stream.write_all(out.as_bytes())?;
    std::io::copy(&mut stdout, stream).context("stream http-backend reply")?;
    let _ = writer.join();
    let _ = child.wait();
    let _ = stream.shutdown(std::net::Shutdown::Write);
    Ok(())
}

fn read_request_head(reader: &mut impl BufRead) -> Result<Option<RequestHead>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    Ok(Some(RequestHead {
        method,
        path,
        headers,
    }))
}

/// Decode a `Transfer-Encoding: chunked` body (git sends these for large POSTs).
fn read_chunked_body(reader: &mut impl BufRead) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line).context("read chunk size")?;
        let size_hex = line.trim().split(';').next().unwrap_or("0");
        let size = usize::from_str_radix(size_hex, 16)
            .with_context(|| format!("bad chunk size {size_hex:?}"))?;
        if size == 0 {
            // Trailers, then the blank line that ends the body.
            loop {
                line.clear();
                if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
                    break;
                }
            }
            return Ok(body);
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader
            .read_exact(&mut body[start..])
            .context("read chunk")?;
        line.clear();
        reader.read_line(&mut line)?;
    }
}

/// The CGI reply's head: `Status:` becomes the HTTP status line, everything else a header.
fn read_cgi_head(reader: &mut impl BufRead) -> Result<(String, Vec<(String, String)>)> {
    let mut status = "200 OK".to_string();
    let mut headers = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        let Some((name, value)) = trimmed.split_once(':') else {
            continue;
        };
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("Status") {
            status = value.to_string();
        } else if !name.eq_ignore_ascii_case("Connection") {
            headers.push((name.to_string(), value.to_string()));
        }
    }
    Ok((status, headers))
}

/// Hermetic CGI environment for `git http-backend` (inherits only `PATH` and `HOME`). Request
/// headers become `HTTP_*` variables, so `Git-Protocol` (v2) and gzip bodies work as on a real
/// server, and `REMOTE_USER` enables push the way an authenticated server does.
fn configure_git_http_backend_cgi(
    cmd: &mut Command,
    project_root: &Path,
    path_info: &str,
    query: &str,
    head: &RequestHead,
    content_length: usize,
) {
    cmd.env_clear();
    if let Ok(path) = env::var("PATH") {
        cmd.env("PATH", path);
    }
    if let Ok(home) = env::var("HOME") {
        cmd.env("HOME", home);
    }
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            crate::bench_env::empty_global_config_path(),
        )
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("GIT_PROJECT_ROOT", project_root)
        .env("PATH_INFO", path_info)
        .env("QUERY_STRING", query)
        .env("REQUEST_METHOD", &head.method)
        .env("SERVER_PROTOCOL", "HTTP/1.1")
        .env("GATEWAY_INTERFACE", "CGI/1.1")
        .env("REMOTE_ADDR", "127.0.0.1")
        .env("REMOTE_USER", "bench");
    if content_length > 0 {
        cmd.env("CONTENT_LENGTH", content_length.to_string());
    }
    if let Some(content_type) = head.header("Content-Type") {
        cmd.env("CONTENT_TYPE", content_type);
    }
    for (name, value) in &head.headers {
        if name.eq_ignore_ascii_case("Content-Type")
            || name.eq_ignore_ascii_case("Content-Length")
            || name.eq_ignore_ascii_case("Transfer-Encoding")
        {
            continue;
        }
        let var = format!("HTTP_{}", name.to_ascii_uppercase().replace('-', "_"));
        cmd.env(var, value);
    }
}

fn git_http_backend_path(git: &Path) -> Result<PathBuf> {
    let out = Command::new(git)
        .arg("--exec-path")
        .output()
        .context("git --exec-path")?;
    if !out.status.success() {
        bail!("git --exec-path failed");
    }
    let exec = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let backend = PathBuf::from(exec).join("git-http-backend");
    if backend.is_file() {
        return Ok(backend);
    }
    find_on_path("git-http-backend").context("locate git-http-backend")
}

fn find_on_path(name: &str) -> Result<PathBuf> {
    let paths = std::env::var_os("PATH").context("PATH unset")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!("`{name}` not found on PATH")
}

/// Child process running `grit-http-server`.
pub struct GritHttpServer {
    child: Child,
    addr: SocketAddr,
}

impl GritHttpServer {
    pub fn start(grit: &Path, http_server: Option<&Path>, root: PathBuf) -> Result<Self> {
        let server_bin = resolve_server_bin(http_server)?;
        let listener = TcpListener::bind("127.0.0.1:0").context("bind grit-http-server")?;
        let addr = listener.local_addr()?;
        drop(listener);

        let mut child = Command::new(&server_bin)
            .arg("--root")
            .arg(&root)
            .arg("--bind")
            .arg(addr.to_string())
            .env("GUST_BIN", grit)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn {}", server_bin.display()))?;

        if !wait_tcp(addr, Duration::from_secs(15)) {
            let _ = child.kill();
            bail!("grit-http-server did not accept connections on {addr}");
        }
        Ok(Self { child, addr })
    }

    pub fn url(&self, repo_name: &str) -> String {
        format!("http://{}/{}", self.addr, repo_name)
    }
}

impl Drop for GritHttpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn cgi_head_maps_status_and_drops_connection() {
        let raw: &[u8] = b"Status: 404 Not Found\r\nContent-Type: text/plain\r\nConnection: keep-alive\r\n\r\nmissing";
        let mut reader = std::io::BufReader::new(raw);
        let (status, headers) = read_cgi_head(&mut reader).expect("cgi head");
        assert_eq!(status, "404 Not Found");
        assert_eq!(
            headers,
            vec![("Content-Type".to_string(), "text/plain".to_string())]
        );
        let mut rest = String::new();
        reader.read_to_string(&mut rest).expect("body");
        assert_eq!(rest, "missing");
    }

    #[test]
    fn chunked_body_is_decoded() {
        let raw: &[u8] = b"5\r\nhello\r\n7;ext=1\r\n, world\r\n0\r\n\r\n";
        let body = read_chunked_body(&mut std::io::BufReader::new(raw)).expect("chunked");
        assert_eq!(body, b"hello, world");
    }

    #[test]
    fn git_http_backend_serves_cloneable_repo() {
        let git = PathBuf::from("/usr/bin/git");
        let tmp = tempfile::tempdir().expect("tempdir");
        let work = tmp.path();
        let bare = work.join("upstream.git");
        Command::new(&git)
            .current_dir(work)
            .args(["init", "--bare", bare.to_str().unwrap()])
            .status()
            .expect("init bare");
        let seed = work.join("seed");
        Command::new(&git)
            .current_dir(work)
            .args(["init", "-q", seed.to_str().unwrap()])
            .status()
            .expect("init seed");
        std::fs::write(seed.join("README"), b"network bench smoke\n").unwrap();
        let seed_steps: &[&[&str]] = &[
            &["add", "README"],
            &["commit", "-m", "seed"],
            &["branch", "-M", "main"],
            &["remote", "add", "origin", bare.to_str().unwrap()],
            &["push", "-q", "origin", "main"],
        ];
        for args in seed_steps {
            let status = Command::new(&git)
                .current_dir(&seed)
                .args(["-c", "user.email=bench@test", "-c", "user.name=bench"])
                .args(*args)
                .status()
                .expect("seed repo setup");
            assert!(status.success(), "seed repo setup step failed");
        }
        let root = work.join("http");
        std::fs::create_dir_all(&root).unwrap();
        Command::new(&git)
            .current_dir(work)
            .args([
                "clone",
                "--bare",
                bare.to_str().unwrap(),
                root.join("fixture.git").to_str().unwrap(),
            ])
            .status()
            .expect("clone bare");
        let server = GitHttpBackendServer::start(&git, root.clone()).expect("start server");
        let url = server.url("fixture.git");
        let dest = work.join("clone");
        let status = Command::new(&git)
            .current_dir(work)
            .args(["clone", "-q", &url, dest.to_str().unwrap()])
            .status()
            .expect("git clone");
        assert!(status.success(), "git clone over git-http-backend failed");
    }
}

pub fn wait_tcp(addr: SocketAddr, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}
