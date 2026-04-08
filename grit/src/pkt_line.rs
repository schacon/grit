//! Git pkt-line format helpers.
//!
//! Implements reading and writing of the pkt-line framing used in the
//! Git wire protocol (both v0 and v2).

use std::io::{self, BufRead, Read, Write};

/// Special packet types.
pub const FLUSH: &str = "0000";
pub const DELIM: &str = "0001";
pub const RESPONSE_END: &str = "0002";

/// Write a single pkt-line to `w`.  The data should NOT include a trailing
/// newline — one is appended automatically.
pub fn write_line(w: &mut impl Write, data: &str) -> io::Result<()> {
    let len = 4 + data.len() + 1; // prefix + content + \n
    writeln!(w, "{:04x}{}", len, data)
}

/// Append a pkt-line encoding of `data` (with trailing newline) to `buf`.
pub fn write_line_to_vec(buf: &mut Vec<u8>, data: &str) -> io::Result<()> {
    let len = 4 + data.len() + 1;
    let line = format!("{:04x}{}\n", len, data);
    buf.extend_from_slice(line.as_bytes());
    Ok(())
}

/// Write a flush packet (`0000`).
pub fn write_flush(w: &mut impl Write) -> io::Result<()> {
    write!(w, "0000")
}

/// Write a delimiter packet (`0001`).
pub fn write_delim(w: &mut impl Write) -> io::Result<()> {
    write!(w, "0001")
}

/// A single packet read from the wire.
#[derive(Debug, PartialEq, Eq)]
pub enum Packet {
    /// A data line (content without trailing newline).
    Data(String),
    /// Flush packet (0000).
    Flush,
    /// Delimiter packet (0001).
    Delim,
    /// Response-end packet (0002).
    ResponseEnd,
}

/// Read one pkt-line from `r`.  Returns `None` at EOF.
pub fn read_packet(r: &mut impl Read) -> io::Result<Option<Packet>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len_str =
        std::str::from_utf8(&len_buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let len = usize::from_str_radix(len_str, 16)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    match len {
        0 => Ok(Some(Packet::Flush)),
        1 => Ok(Some(Packet::Delim)),
        2 => Ok(Some(Packet::ResponseEnd)),
        n if n <= 4 => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid pkt-line length: {n}"),
        )),
        n => {
            let payload_len = n - 4;
            let mut buf = vec![0u8; payload_len];
            r.read_exact(&mut buf)?;
            // NUL is valid UTF-8; avoid lossy conversion (would break ref advertisement \0).
            let s = String::from_utf8(buf)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(Some(Packet::Data(
                s.strip_suffix('\n').unwrap_or(&s).to_owned(),
            )))
        }
    }
}

/// Read packets from `r` until a flush or delimiter packet, or EOF.
/// Returns the collected data lines and the terminator (Flush/Delim/None).
pub fn read_until_flush_or_delim(r: &mut impl Read) -> io::Result<(Vec<String>, Option<Packet>)> {
    let mut lines = Vec::new();
    loop {
        match read_packet(r)? {
            None => return Ok((lines, None)),
            Some(Packet::Flush) => return Ok((lines, Some(Packet::Flush))),
            Some(Packet::Delim) => return Ok((lines, Some(Packet::Delim))),
            Some(Packet::ResponseEnd) => return Ok((lines, Some(Packet::ResponseEnd))),
            Some(Packet::Data(s)) => lines.push(s),
        }
    }
}

/// `grit pkt-line pack`: read text lines from stdin, write pkt-line to stdout.
pub fn cmd_pack() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        match line.as_str() {
            "0000" => write_flush(&mut out)?,
            "0001" => write_delim(&mut out)?,
            "0002" => write!(out, "0002")?,
            _ => write_line(&mut out, &line)?,
        }
    }
    out.flush()
}

/// `grit pkt-line unpack`: read pkt-line from stdin, write text lines to stdout.
pub fn cmd_unpack() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut out = stdout.lock();

    loop {
        match read_packet(&mut input)? {
            None => break,
            Some(Packet::Flush) => writeln!(out, "0000")?,
            Some(Packet::Delim) => writeln!(out, "0001")?,
            Some(Packet::ResponseEnd) => writeln!(out, "0002")?,
            Some(Packet::Data(s)) => writeln!(out, "{s}")?,
        }
    }
    out.flush()
}

fn write_sideband_packet(w: &mut impl Write, band: u8, payload: &[u8]) -> io::Result<()> {
    let len = 4 + 1 + payload.len();
    write!(w, "{len:04x}")?;
    w.write_all(&[band])?;
    w.write_all(payload)?;
    Ok(())
}

/// `grit pkt-line send-split-sideband`:
/// emit a pkt-line stream containing channel-1 and channel-2 sideband data.
pub fn cmd_send_split_sideband() -> io::Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Primary output payload.
    write_sideband_packet(&mut out, 1, b"primary: regular output\n")?;

    // Progress payloads, with "Hello, world!\n" intentionally split across
    // two packets to exercise reassembly behavior in readers.
    write_sideband_packet(&mut out, 2, b"Foo.\n")?;
    write_sideband_packet(&mut out, 2, b"Bar.\n")?;
    write_sideband_packet(&mut out, 2, b"Hello, ")?;
    write_sideband_packet(&mut out, 2, b"world!\n")?;

    write_flush(&mut out)?;
    out.flush()
}

fn parse_hex_len(prefix: &[u8]) -> io::Result<usize> {
    let s = std::str::from_utf8(prefix)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e}")))?;
    usize::from_str_radix(s, 16)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("invalid length: {e}")))
}

/// `grit pkt-line receive-sideband`:
/// decode sideband progress payloads and print them to stderr.
pub fn cmd_receive_sideband() -> io::Result<()> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    let stderr = io::stderr();
    let mut err = stderr.lock();

    let mut i = 0usize;
    let mut progress_buf: Vec<u8> = Vec::new();
    while i < input.len() {
        if i + 4 > input.len() {
            writeln!(err, "unexpected disconnect while reading sideband packet")?;
            break;
        }
        let len = parse_hex_len(&input[i..i + 4])?;
        i += 4;

        if len == 0 {
            break;
        }
        if len <= 4 {
            writeln!(err, "missing sideband designator")?;
            continue;
        }
        let payload_len = len - 4;
        if i + payload_len > input.len() {
            writeln!(err, "unexpected disconnect while reading sideband packet")?;
            break;
        }
        let payload = &input[i..i + payload_len];
        i += payload_len;

        let band = payload[0];
        let data = &payload[1..];
        if band == 2 || band == 3 {
            progress_buf.extend_from_slice(data);
            while let Some(pos) = progress_buf.iter().position(|b| *b == b'\n') {
                let line = String::from_utf8_lossy(&progress_buf[..=pos]);
                write!(err, "{line}")?;
                progress_buf.drain(..=pos);
            }
        }
    }

    if !progress_buf.is_empty() {
        writeln!(err, "{}", String::from_utf8_lossy(&progress_buf))?;
    }
    err.flush()
}

/// `grit pkt-line unpack-sideband`:
/// separate channel-1 output (stdout) from channel-2/3 progress (stderr).
pub fn cmd_unpack_sideband(chomp_newline: bool, reader_use_sideband: bool) -> io::Result<()> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    let stdout = io::stdout();
    let stderr = io::stderr();
    let mut out = stdout.lock();
    let mut err = stderr.lock();

    let mut i = 0usize;
    let mut progress_buf: Vec<u8> = Vec::new();

    while i < input.len() {
        if i + 4 > input.len() {
            break;
        }
        let len = parse_hex_len(&input[i..i + 4])?;
        i += 4;
        if len == 0 {
            break;
        }
        if len <= 4 {
            continue;
        }
        let payload_len = len - 4;
        if i + payload_len > input.len() {
            break;
        }
        let payload = &input[i..i + payload_len];
        i += payload_len;

        let band = payload[0];
        let data = &payload[1..];

        match band {
            1 => {
                if chomp_newline {
                    let trimmed = if data.ends_with(b"\n") {
                        &data[..data.len() - 1]
                    } else {
                        data
                    };
                    out.write_all(trimmed)?;
                } else {
                    out.write_all(data)?;
                }
            }
            2 | 3 => {
                if reader_use_sideband {
                    progress_buf.extend_from_slice(data);
                    while let Some(pos) = progress_buf.iter().position(|b| *b == b'\n') {
                        let mut line = progress_buf[..pos].to_vec();
                        if line.ends_with(b"\r") {
                            line.pop();
                        }
                        writeln!(err, "remote: {}        ", String::from_utf8_lossy(&line))?;
                        progress_buf.drain(..=pos);
                    }
                } else if chomp_newline {
                    let trimmed = if data.ends_with(b"\n") {
                        &data[..data.len() - 1]
                    } else {
                        data
                    };
                    err.write_all(trimmed)?;
                } else {
                    err.write_all(data)?;
                }
            }
            _ => {}
        }
    }

    if reader_use_sideband && !progress_buf.is_empty() {
        writeln!(
            err,
            "remote: {}        ",
            String::from_utf8_lossy(&progress_buf)
        )?;
    }

    out.flush()?;
    err.flush()
}
