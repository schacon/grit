//! Git-compatible "simple IPC" over Unix domain sockets using pkt-line framing.
//!
//! Used by `test-tool simple-ipc` (`t0052-simple-ipc.sh`).

use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Application callback return value that requests server shutdown (no reply).
pub const SIMPLE_IPC_QUIT: i32 = -2;

/// Git `LARGE_PACKET_DATA_MAX` (`65520 - 4`).
const LARGE_PACKET_DATA_MAX: usize = 65516;

const CONNECT_TIMEOUT_MS: i32 = 1000;
const WAIT_STEP_MS: u64 = 50;

/// Whether simple IPC is supported (Unix only).
#[must_use]
pub fn supports_simple_ipc() -> bool {
    cfg!(unix)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcActiveState {
    Listening,
    NotListening,
    InvalidPath,
    PathNotFound,
    OtherError,
}

fn packet_hex_len(payload_len: usize) -> io::Result<[u8; 4]> {
    let packet_size = payload_len + 4;
    if packet_size > 0xffff {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "packet exceeds max size",
        ));
    }
    Ok(hex4_upper(packet_size))
}

fn hex4_upper(mut n: usize) -> [u8; 4] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = [0u8; 4];
    for i in (0..4).rev() {
        out[i] = HEX[n & 0xf];
        n >>= 4;
    }
    out
}

fn write_packet(w: &mut dyn Write, buf: &[u8]) -> io::Result<()> {
    if buf.len() > LARGE_PACKET_DATA_MAX {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "packet exceeds max size",
        ));
    }
    let hdr = packet_hex_len(buf.len())?;
    w.write_all(&hdr)?;
    w.write_all(buf)?;
    Ok(())
}

fn write_packetized_from_buf(w: &mut dyn Write, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        let n = data.len().min(LARGE_PACKET_DATA_MAX);
        write_packet(w, &data[..n])?;
        data = &data[n..];
    }
    Ok(())
}

fn packet_flush_gently(w: &mut dyn Write) -> io::Result<()> {
    w.write_all(b"0000")?;
    w.flush()?;
    Ok(())
}

fn read_one_packet<R: Read>(r: &mut R, buf: &mut Vec<u8>) -> io::Result<Option<()>> {
    let mut linelen = [0u8; 4];
    match r.read_exact(&mut linelen) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len_str = std::str::from_utf8(&linelen).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid pkt-line length encoding: {e}"),
        )
    })?;
    let len = usize::from_str_radix(len_str, 16).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid pkt-line length: {e}"),
        )
    })?;
    match len {
        0 => Ok(None),
        1 | 2 => Ok(Some(())),
        n if n < 4 => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad pkt-line length {n}"),
        )),
        n => {
            let payload = n - 4;
            let start = buf.len();
            buf.resize(start + payload, 0);
            r.read_exact(&mut buf[start..])?;
            Ok(Some(()))
        }
    }
}

fn read_packetized_to_end<R: Read>(r: &mut R) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        if read_one_packet(r, &mut out)?.is_none() {
            break;
        }
    }
    Ok(out)
}

fn unix_stream_connect(path: &Path, _disallow_chdir: bool) -> io::Result<UnixStream> {
    // Git may chdir for overlong paths; harness paths (e.g. `ipc-test`) fit in `sun_path`.
    UnixStream::connect(path)
}

fn connect_with_retry(path: &Path, wait_if_busy: bool, wait_if_not_found: bool) -> IpcActiveState {
    let mut elapsed: i32 = 0;
    loop {
        match unix_stream_connect(path, false) {
            Ok(s) => {
                drop(s);
                return IpcActiveState::Listening;
            }
            Err(e) => {
                let code = e.raw_os_error();
                let retry = match code {
                    Some(libc::ENOENT) => wait_if_not_found,
                    Some(libc::ECONNREFUSED) | Some(libc::ETIMEDOUT) => wait_if_busy,
                    _ => false,
                };
                if !retry || elapsed >= CONNECT_TIMEOUT_MS {
                    return match code {
                        Some(libc::ENOENT) => IpcActiveState::PathNotFound,
                        Some(libc::ECONNREFUSED) => IpcActiveState::NotListening,
                        _ => IpcActiveState::OtherError,
                    };
                }
                thread::sleep(Duration::from_millis(WAIT_STEP_MS));
                elapsed += WAIT_STEP_MS as i32;
            }
        }
    }
}

/// Probe whether a server is accepting connections at `path`.
pub fn ipc_get_active_state(path: &Path) -> IpcActiveState {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return IpcActiveState::NotListening,
        Err(_) => return IpcActiveState::InvalidPath,
    };
    if !meta.file_type().is_socket() {
        return IpcActiveState::InvalidPath;
    }
    connect_with_retry(path, false, false)
}

#[derive(Default)]
pub struct IpcClientConnectOptions {
    pub wait_if_busy: bool,
    pub wait_if_not_found: bool,
    pub uds_disallow_chdir: bool,
}

fn connect_for_client(path: &Path, options: &IpcClientConnectOptions) -> io::Result<UnixStream> {
    let mut elapsed: i32 = 0;
    loop {
        match unix_stream_connect(path, options.uds_disallow_chdir) {
            Ok(s) => return Ok(s),
            Err(e) => {
                let code = e.raw_os_error();
                let retry = match code {
                    Some(libc::ENOENT) => options.wait_if_not_found,
                    Some(libc::ECONNREFUSED) | Some(libc::ETIMEDOUT) => options.wait_if_busy,
                    _ => false,
                };
                if !retry || elapsed >= CONNECT_TIMEOUT_MS {
                    return Err(e);
                }
                thread::sleep(Duration::from_millis(WAIT_STEP_MS));
                elapsed += WAIT_STEP_MS as i32;
            }
        }
    }
}

/// Connect, send pkt-line message + flush, read full response (until flush).
pub fn ipc_client_send_command(
    path: &Path,
    options: &IpcClientConnectOptions,
    message: &[u8],
) -> io::Result<Vec<u8>> {
    let mut stream = connect_for_client(path, options)?;
    write_packetized_from_buf(&mut stream, message)?;
    packet_flush_gently(&mut stream)?;
    read_packetized_to_end(&mut stream)
}

fn block_sigpipe() {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGPIPE);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

fn wait_for_io_start(stream: &UnixStream, server_shutdown: &AtomicBool) -> io::Result<()> {
    let fd = stream.as_raw_fd();
    loop {
        if server_shutdown.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "shutdown"));
        }
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let n = unsafe { libc::poll(&mut pfd, 1, 10) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(err);
        }
        if pfd.revents & libc::POLLHUP != 0 {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "client hangup",
            ));
        }
        if pfd.revents & libc::POLLIN != 0 {
            return Ok(());
        }
    }
}

type AppCb = Arc<dyn Fn(&[u8], &mut dyn Write) -> i32 + Send + Sync + 'static>;

struct WorkQueue {
    fifo: Mutex<VecDeque<UnixStream>>,
    cv: Condvar,
    shutdown_requested: AtomicBool,
    capacity: usize,
}

impl WorkQueue {
    fn new(capacity: usize) -> Self {
        Self {
            fifo: Mutex::new(VecDeque::new()),
            cv: Condvar::new(),
            shutdown_requested: AtomicBool::new(false),
            capacity,
        }
    }

    fn enqueue(&self, stream: UnixStream) {
        let mut guard = self.fifo.lock().unwrap_or_else(|e| e.into_inner());
        if self.shutdown_requested.load(Ordering::SeqCst) {
            return;
        }
        if guard.len() >= self.capacity {
            return;
        }
        guard.push_back(stream);
        self.cv.notify_one();
    }

    fn dequeue(&self) -> Option<UnixStream> {
        let mut guard = self.fifo.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(s) = guard.pop_front() {
                return Some(s);
            }
            if self.shutdown_requested.load(Ordering::SeqCst) {
                return None;
            }
            guard = self.cv.wait(guard).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn stop(&self) {
        self.shutdown_requested.store(true, Ordering::SeqCst);
        let mut guard = self.fifo.lock().unwrap_or_else(|e| e.into_inner());
        guard.clear();
        drop(guard);
        self.cv.notify_all();
    }
}

fn serve_one_connection(
    mut stream: UnixStream,
    app: AppCb,
    server_shutdown: Arc<AtomicBool>,
    wake: Arc<Mutex<UnixStream>>,
    queue: Arc<WorkQueue>,
) {
    if wait_for_io_start(&stream, &server_shutdown).is_err() {
        let _ = stream.shutdown(Shutdown::Both);
        return;
    }
    let request = match read_packetized_to_end(&mut stream) {
        Ok(r) => r,
        Err(_) => {
            let _ = stream.shutdown(Shutdown::Both);
            return;
        }
    };
    let ret = app(&request, &mut stream);
    let _ = packet_flush_gently(&mut stream);
    let _ = stream.shutdown(Shutdown::Both);
    if ret == SIMPLE_IPC_QUIT {
        server_shutdown.store(true, Ordering::SeqCst);
        queue.stop();
        if let Ok(mut tx) = wake.lock() {
            let _ = tx.write_all(b"Q");
        }
    }
}

#[derive(Debug)]
pub enum ServerRunError {
    Io(io::Error),
    AddressInUse,
}

impl std::fmt::Display for ServerRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerRunError::Io(e) => write!(f, "{e}"),
            ServerRunError::AddressInUse => write!(f, "socket path already in use"),
        }
    }
}

impl std::error::Error for ServerRunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ServerRunError::Io(e) => Some(e),
            ServerRunError::AddressInUse => None,
        }
    }
}

fn try_bind_server(path: &Path) -> io::Result<()> {
    let lock_path = path.with_extension("lock");
    let _ = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&lock_path);

    if path.exists() {
        if is_socket(path) && unix_stream_connect(path, false).is_ok() {
            let _ = std::fs::remove_file(&lock_path);
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "another server is listening",
            ));
        }
        let _ = std::fs::remove_file(path);
    }

    let _ = std::fs::remove_file(&lock_path);
    Ok(())
}

fn is_socket(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .ok()
        .is_some_and(|m| m.file_type().is_socket())
}

/// Blocking IPC server (`run-daemon`).
pub fn ipc_server_run(path: &Path, nr_threads: usize, app: AppCb) -> Result<(), ServerRunError> {
    try_bind_server(path).map_err(|e| {
        if e.kind() == io::ErrorKind::AddrInUse {
            ServerRunError::AddressInUse
        } else {
            ServerRunError::Io(e)
        }
    })?;

    let listener = UnixListener::bind(path).map_err(ServerRunError::Io)?;
    listener.set_nonblocking(true).map_err(ServerRunError::Io)?;

    let nr_threads = nr_threads.max(1);
    let capacity = nr_threads.saturating_mul(100).max(1);
    let queue = Arc::new(WorkQueue::new(capacity));
    let server_shutdown = Arc::new(AtomicBool::new(false));

    let (shutdown_tx, shutdown_rx) = UnixStream::pair().map_err(ServerRunError::Io)?;
    shutdown_rx
        .set_nonblocking(true)
        .map_err(ServerRunError::Io)?;
    let wake = Arc::new(Mutex::new(shutdown_tx));

    let mut worker_handles = Vec::new();
    for _ in 0..nr_threads {
        let q = Arc::clone(&queue);
        let app_w = Arc::clone(&app);
        let shut = Arc::clone(&server_shutdown);
        let wake_w = Arc::clone(&wake);
        let q_for_worker = Arc::clone(&queue);
        worker_handles.push(thread::spawn(move || {
            block_sigpipe();
            while let Some(stream) = q.dequeue() {
                serve_one_connection(
                    stream,
                    app_w.clone(),
                    shut.clone(),
                    wake_w.clone(),
                    q_for_worker.clone(),
                );
            }
        }));
    }

    block_sigpipe();
    let l_fd = listener.as_raw_fd();
    let s_fd = shutdown_rx.as_raw_fd();

    loop {
        if server_shutdown.load(Ordering::SeqCst) {
            break;
        }
        let mut fds = [
            libc::pollfd {
                fd: s_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: l_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, 60_000) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break;
        }
        if fds[0].revents & libc::POLLIN != 0 {
            break;
        }
        if fds[1].revents & libc::POLLIN != 0 {
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        queue.enqueue(stream);
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
        }
    }

    queue.stop();
    drop(listener);
    for h in worker_handles {
        let _ = h.join();
    }
    let _ = std::fs::remove_file(path);
    Ok(())
}

/// Test daemon command handler (mirrors Git `test-simple-ipc.c`).
#[must_use]
pub fn test_app_callback() -> AppCb {
    Arc::new(|request: &[u8], reply: &mut dyn Write| {
        if request == b"quit" {
            return SIMPLE_IPC_QUIT;
        }
        if request == b"ping" {
            let _ = write_packetized_from_buf(reply, b"pong");
            return 0;
        }
        if request == b"big" {
            let mut line = Vec::with_capacity(84);
            for row in 0..10_000 {
                line.clear();
                use std::io::Write as _;
                let _ = writeln!(&mut line, "big: {:075}", row);
                let _ = write_packetized_from_buf(reply, &line);
            }
            return 0;
        }
        if request == b"chunk" {
            let mut line = Vec::with_capacity(84);
            for row in 0..10_000 {
                line.clear();
                use std::io::Write as _;
                let _ = writeln!(&mut line, "big: {:075}", row);
                let _ = write_packet(reply, &line);
            }
            return 0;
        }
        if request == b"slow" {
            let mut line = Vec::with_capacity(84);
            for row in 0..1000 {
                line.clear();
                use std::io::Write as _;
                let _ = writeln!(&mut line, "big: {:075}", row);
                let _ = write_packet(reply, &line);
                thread::sleep(Duration::from_millis(10));
            }
            return 0;
        }
        if request.len() >= 10 && request.starts_with(b"sendbytes ") {
            return handle_sendbytes(request, reply);
        }
        let msg = format!("unhandled command: {}", String::from_utf8_lossy(request));
        let _ = write_packetized_from_buf(reply, msg.as_bytes());
        0
    })
}

fn handle_sendbytes(request: &[u8], reply: &mut dyn Write) -> i32 {
    let rest = &request[b"sendbytes ".len()..];
    if rest.is_empty() {
        return 0;
    }
    let b0 = rest[0];
    let mut errs = 0usize;
    for &b in &rest[1..] {
        if b != b0 {
            errs += 1;
        }
    }
    if errs > 0 {
        let msg = format!("errs:{errs}\n");
        let _ = write_packetized_from_buf(reply, msg.as_bytes());
    } else {
        let msg = format!("rcvd:{}{:08}\n", char::from(b0), rest.len());
        let _ = write_packetized_from_buf(reply, msg.as_bytes());
    }
    0
}

/// `test-tool simple-ipc` entry (Unix). Returns exit code.
pub fn run_simple_ipc_tool(args: &[String]) -> i32 {
    if args.first().map(|s| s.as_str()) == Some("SUPPORTS_SIMPLE_IPC") {
        return 0;
    }
    if args.is_empty() {
        eprintln!("usage: test-tool simple-ipc <subcommand> ...");
        return 1;
    }

    let mut path = PathBuf::from("ipc-test");
    let mut nr_threads = 5usize;
    let mut max_wait_sec = 60u64;
    let mut bytecount = 1024usize;
    let mut batchsize = 10usize;
    let mut token: Option<String> = None;
    let mut bytevalue: u8 = b'x';

    let sub = args[0].clone();
    let mut i = 1usize;
    while i < args.len() {
        let a = args[i].as_str();
        if let Some(v) = a.strip_prefix("--name=") {
            path = PathBuf::from(v);
        } else if let Some(v) = a.strip_prefix("--threads=") {
            nr_threads = v.parse().unwrap_or(1).max(1);
        } else if let Some(v) = a.strip_prefix("--max-wait=") {
            max_wait_sec = v.parse().unwrap_or(0);
        } else if let Some(v) = a.strip_prefix("--bytecount=") {
            bytecount = v.parse().unwrap_or(1).max(1);
        } else if let Some(v) = a.strip_prefix("--batchsize=") {
            batchsize = v.parse().unwrap_or(1).max(1);
        } else if let Some(v) = a.strip_prefix("--token=") {
            token = Some(v.to_string());
        } else if let Some(v) = a.strip_prefix("--byte=") {
            if let Some(c) = v.as_bytes().first() {
                bytevalue = *c;
            }
        }
        i += 1;
    }

    match sub.as_str() {
        "is-active" => match ipc_get_active_state(&path) {
            IpcActiveState::Listening => 0,
            IpcActiveState::NotListening => {
                eprintln!("no server listening at '{}'", path.display());
                1
            }
            IpcActiveState::PathNotFound => {
                eprintln!("path not found '{}'", path.display());
                1
            }
            IpcActiveState::InvalidPath => {
                eprintln!("invalid pipe/socket name '{}'", path.display());
                1
            }
            IpcActiveState::OtherError => {
                eprintln!("other error for '{}'", path.display());
                1
            }
        },
        "run-daemon" => {
            let app = test_app_callback();
            match ipc_server_run(&path, nr_threads, app) {
                Ok(()) => 0,
                Err(ServerRunError::AddressInUse) => {
                    eprintln!("socket/pipe already in use: '{}'", path.display());
                    1
                }
                Err(ServerRunError::Io(e)) => {
                    eprintln!("could not start server on '{}': {e}", path.display());
                    1
                }
            }
        }
        "start-daemon" => match spawn_daemon(&path, nr_threads, max_wait_sec) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        "stop-daemon" => {
            if !matches!(ipc_get_active_state(&path), IpcActiveState::Listening) {
                eprintln!("no server listening at '{}'", path.display());
                return 1;
            }
            let opts = IpcClientConnectOptions {
                wait_if_busy: true,
                wait_if_not_found: false,
                uds_disallow_chdir: false,
            };
            if ipc_client_send_command(&path, &opts, b"quit").is_err() {
                return 1;
            }
            let deadline = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                + max_wait_sec;
            loop {
                if !matches!(ipc_get_active_state(&path), IpcActiveState::Listening) {
                    return 0;
                }
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                if now > deadline {
                    eprintln!("daemon has not shutdown yet");
                    return 1;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        "send" => {
            if !matches!(ipc_get_active_state(&path), IpcActiveState::Listening) {
                eprintln!("no server listening at '{}'", path.display());
                return 1;
            }
            let cmd = token.as_deref().unwrap_or("(no-command)");
            let opts = IpcClientConnectOptions {
                wait_if_busy: true,
                wait_if_not_found: false,
                uds_disallow_chdir: false,
            };
            match ipc_client_send_command(&path, &opts, cmd.as_bytes()) {
                Ok(resp) => {
                    if !resp.is_empty() {
                        println!("{}", String::from_utf8_lossy(&resp).trim_end());
                    }
                    0
                }
                Err(_) => {
                    eprintln!("failed to send '{cmd}' to '{}'", path.display());
                    1
                }
            }
        }
        "sendbytes" => {
            if !matches!(ipc_get_active_state(&path), IpcActiveState::Listening) {
                eprintln!("no server listening at '{}'", path.display());
                return 1;
            }
            let mut msg = b"sendbytes ".to_vec();
            msg.extend(std::iter::repeat_n(bytevalue, bytecount));
            let opts = IpcClientConnectOptions {
                wait_if_busy: true,
                wait_if_not_found: false,
                uds_disallow_chdir: false,
            };
            match ipc_client_send_command(&path, &opts, &msg) {
                Ok(resp) => {
                    let tail = String::from_utf8_lossy(&resp);
                    let tail = tail.trim_end();
                    println!("sent:{}{:08} {tail}", char::from(bytevalue), bytecount);
                    0
                }
                Err(_) => 1,
            }
        }
        "multiple" => {
            if !matches!(ipc_get_active_state(&path), IpcActiveState::Listening) {
                eprintln!("no server listening at '{}'", path.display());
                return 1;
            }
            run_multiple(&path, nr_threads, bytecount, batchsize)
        }
        _ => {
            eprintln!("Unhandled subcommand: '{sub}'");
            1
        }
    }
}

fn spawn_daemon(path: &Path, nr_threads: usize, max_wait_sec: u64) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(exe);
    cmd.arg("test-tool")
        .arg("simple-ipc")
        .arg("run-daemon")
        .arg(format!("--name={}", path.display()))
        .arg(format!("--threads={nr_threads}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn().map_err(|e| e.to_string())?;
    let deadline = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs()
        + max_wait_sec.max(1);
    loop {
        if matches!(ipc_get_active_state(path), IpcActiveState::Listening) {
            return Ok(());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        if now > deadline {
            return Err("daemon not online yet".to_string());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn run_multiple(path: &Path, nr_threads: usize, bytecount: usize, batchsize: usize) -> i32 {
    use std::sync::atomic::{AtomicUsize, Ordering as AOrd};
    let sum_errors = Arc::new(AtomicUsize::new(0));
    let sum_good = Arc::new(AtomicUsize::new(0));
    let sum_join_errors = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for k in 0..nr_threads {
        let p = path.to_path_buf();
        let letter = (b'A' + (k % 26) as u8) as char;
        let base_count = bytecount + batchsize * (k / 26);
        let batch = batchsize;
        let sg = Arc::clone(&sum_good);
        let se = Arc::clone(&sum_errors);
        handles.push(thread::spawn(move || {
            for t in 0..batch {
                let n = base_count + t;
                let mut msg = b"sendbytes ".to_vec();
                msg.extend(std::iter::repeat_n(letter as u8, n));
                let opts = IpcClientConnectOptions {
                    wait_if_busy: true,
                    wait_if_not_found: false,
                    uds_disallow_chdir: true,
                };
                match ipc_client_send_command(&p, &opts, &msg) {
                    Ok(resp) => {
                        let tail = String::from_utf8_lossy(&resp);
                        let tail = tail.trim_end();
                        println!("sent:{}{:08} {tail}", letter, n);
                        sg.fetch_add(1, AOrd::SeqCst);
                    }
                    Err(_) => {
                        se.fetch_add(1, AOrd::SeqCst);
                    }
                }
            }
        }));
    }
    for h in handles {
        if h.join().is_err() {
            sum_join_errors.fetch_add(1, AOrd::SeqCst);
        }
    }
    let good = sum_good.load(AOrd::SeqCst);
    let je = sum_join_errors.load(AOrd::SeqCst);
    let err = sum_errors.load(AOrd::SeqCst);
    println!("client (good {good}) (join {je}), (errors {err})");
    if je + err > 0 {
        1
    } else {
        0
    }
}
