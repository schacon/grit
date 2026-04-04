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
    write!(w, "{:04x}{}\n", len, data)
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
    let len_str = std::str::from_utf8(&len_buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
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
pub fn read_until_flush_or_delim(
    r: &mut impl Read,
) -> io::Result<(Vec<String>, Option<Packet>)> {
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
