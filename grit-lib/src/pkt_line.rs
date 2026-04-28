//! Git pkt-line and sideband wire-format helpers.

use std::io::{self, Read, Write};

/// Flush packet marker.
pub const FLUSH: &str = "0000";
/// Delimiter packet marker.
pub const DELIM: &str = "0001";
/// Response-end packet marker.
pub const RESPONSE_END: &str = "0002";

/// A single packet read from the wire.
#[derive(Debug, PartialEq, Eq)]
pub enum Packet {
    /// A data line, decoded lossily as UTF-8 and without a trailing newline.
    Data(String),
    /// Flush packet (`0000`).
    Flush,
    /// Delimiter packet (`0001`).
    Delim,
    /// Response-end packet (`0002`).
    ResponseEnd,
}

/// Write a single pkt-line to `w`, appending a trailing newline.
pub fn write_line(w: &mut impl Write, data: &str) -> io::Result<()> {
    let len = 4 + data.len() + 1;
    writeln!(w, "{len:04x}{data}")
}

/// Append a pkt-line encoding of `data` to `buf`, appending a trailing newline.
pub fn write_line_to_vec(buf: &mut Vec<u8>, data: &str) -> io::Result<()> {
    let len = 4 + data.len() + 1;
    let line = format!("{len:04x}{data}\n");
    buf.extend_from_slice(line.as_bytes());
    Ok(())
}

/// Write a flush packet.
pub fn write_flush(w: &mut impl Write) -> io::Result<()> {
    write!(w, "{FLUSH}")
}

/// Write a delimiter packet.
pub fn write_delim(w: &mut impl Write) -> io::Result<()> {
    write!(w, "{DELIM}")
}

/// Write one pkt-line with arbitrary bytes and no appended newline.
pub fn write_packet_raw(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    let total = payload
        .len()
        .checked_add(4)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "pkt-line payload too large"))?;
    if total > 65520 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pkt-line exceeds maximum size",
        ));
    }
    write!(w, "{total:04x}")?;
    w.write_all(payload)
}

/// Read one pkt-line from `r`, returning `None` at EOF.
pub fn read_packet(r: &mut impl Read) -> io::Result<Option<Packet>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = parse_hex_len(&len_buf)?;

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
            let s = String::from_utf8_lossy(&buf).into_owned();
            Ok(Some(Packet::Data(
                s.strip_suffix('\n').unwrap_or(&s).to_owned(),
            )))
        }
    }
}

/// Read packets until a flush, delimiter, response-end, or EOF.
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

/// Read data pkt-lines until a flush packet.
pub fn read_data_lines_until_flush(
    r: &mut impl Read,
    err_not_flush: &str,
) -> io::Result<Vec<String>> {
    let mut lines = Vec::new();
    loop {
        match read_packet(r)? {
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    err_not_flush.to_string(),
                ));
            }
            Some(Packet::Flush) => return Ok(lines),
            Some(Packet::Delim | Packet::ResponseEnd) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    err_not_flush.to_string(),
                ));
            }
            Some(Packet::Data(s)) => lines.push(s),
        }
    }
}

/// Write payload on sideband channel 1 using Git's side-band-64k chunking.
pub fn write_sideband_channel1_64k(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    const MAX_PAYLOAD: usize = 65515;
    for chunk in payload.chunks(MAX_PAYLOAD) {
        write_sideband_packet(w, 1, chunk)?;
    }
    Ok(())
}

/// Decode a sideband pkt-line stream into the raw primary channel payload.
pub fn decode_sideband_primary(mut input: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    while !input.is_empty() {
        if input.len() < 4 {
            break;
        }
        let len = parse_hex_len(&input[..4])?;
        input = &input[4..];
        if len == 0 {
            break;
        }
        if len <= 4 || input.len() < len - 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated sideband packet",
            ));
        }
        let payload_len = len - 4;
        let payload = &input[..payload_len];
        input = &input[payload_len..];
        if let Some((&1, data)) = payload.split_first() {
            out.extend_from_slice(data);
        }
    }
    Ok(out)
}

fn write_sideband_packet(w: &mut impl Write, band: u8, payload: &[u8]) -> io::Result<()> {
    let len = 4 + 1 + payload.len();
    write!(w, "{len:04x}")?;
    w.write_all(&[band])?;
    w.write_all(payload)
}

fn parse_hex_len(prefix: &[u8]) -> io::Result<usize> {
    let s = std::str::from_utf8(prefix)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e}")))?;
    usize::from_str_radix(s, 16)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("invalid length: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_basic_packets() {
        let mut out = Vec::new();
        write_line_to_vec(&mut out, "hello").unwrap();
        write_delim(&mut out).unwrap();
        write_flush(&mut out).unwrap();

        let mut cur = out.as_slice();
        assert_eq!(
            read_packet(&mut cur).unwrap(),
            Some(Packet::Data("hello".into()))
        );
        assert_eq!(read_packet(&mut cur).unwrap(), Some(Packet::Delim));
        assert_eq!(read_packet(&mut cur).unwrap(), Some(Packet::Flush));
    }

    #[test]
    fn decodes_sideband_primary() {
        let mut out = Vec::new();
        write_sideband_channel1_64k(&mut out, b"abc").unwrap();
        write_flush(&mut out).unwrap();

        assert_eq!(decode_sideband_primary(&out).unwrap(), b"abc");
    }
}
