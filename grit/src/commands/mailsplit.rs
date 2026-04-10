//! `grit mailsplit` — split mbox into individual messages.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Debug, ClapArgs)]
#[command(
    about = "Split mbox into individual messages",
    override_usage = "grit mailsplit [-d<prec>] [-f<n>] [-b] [--keep-cr] [--mboxrd] -o<dir> [<mbox>...]"
)]
pub struct Args {
    #[arg(short = 'o', long = "output")]
    pub output: Option<PathBuf>,

    #[arg(long = "keep-cr")]
    pub keep_cr: bool,

    /// Filename width (3–9), not a skip count (matches Git `-d`).
    #[arg(short = 'd', default_value = "4")]
    pub nr_prec: u32,

    /// Initial counter for numbering (matches Git `-f`; first file is counter + 1).
    #[arg(short = 'f', default_value = "0")]
    pub first_nr: u32,

    #[arg(short = 'b', long)]
    pub allow_bare: bool,

    #[arg(long = "mboxrd")]
    pub mboxrd: bool,

    #[arg(value_name = "MBOX")]
    pub mbox: Vec<PathBuf>,
}

pub fn run(args: Args) -> Result<()> {
    if !(3..10).contains(&args.nr_prec) {
        bail!("mailsplit: -d must be between 3 and 9");
    }

    let (dir, files) = resolve_paths(&args)?;

    fs::create_dir_all(&dir).with_context(|| format!("creating {:?}", dir))?;

    let mut seq = args.first_nr;
    let mut total_written = 0u32;

    for file in files {
        let data = if file.as_os_str() == "-" {
            let mut v = Vec::new();
            std::io::stdin()
                .read_to_end(&mut v)
                .context("reading stdin")?;
            v
        } else {
            fs::read(&file).with_context(|| format!("reading {:?}", file))?
        };

        if data.is_empty() && file.as_os_str() != "-" {
            bail!("empty mbox: '{}'", file.display());
        }

        let mut pos = skip_leading_ws(&data);
        if pos >= data.len() {
            if file.as_os_str() == "-" {
                println!("{total_written}");
                return Ok(());
            }
            bail!("empty mbox: '{}'", file.display());
        }
        pos = skip_preamble_to_first_from(&data, pos, args.keep_cr);

        let mut file_done = false;
        while !file_done {
            seq += 1;
            let name = format!("{:0width$}", seq, width = args.nr_prec as usize);
            let path = dir.join(&name);
            let mut out =
                fs::File::create_new(&path).with_context(|| format!("creating {:?}", path))?;
            file_done = split_one_message(
                &data,
                &mut pos,
                &mut out,
                args.allow_bare,
                args.keep_cr,
                args.mboxrd,
            )?;
            total_written += 1;
        }
    }

    println!("{total_written}");
    Ok(())
}

fn resolve_paths(args: &Args) -> Result<(PathBuf, Vec<PathBuf>)> {
    match (&args.output, args.mbox.len()) {
        (Some(dir), 0) => Ok((dir.clone(), vec![PathBuf::from("-")])),
        (Some(dir), _) => Ok((dir.clone(), args.mbox.clone())),
        (None, 1) => Ok((args.mbox[0].clone(), vec![PathBuf::from("-")])),
        (None, 2) => Ok((args.mbox[1].clone(), vec![args.mbox[0].clone()])),
        _ => bail!("mailsplit: need -o<dir> or legacy <mbox> <dir> usage"),
    }
}

fn skip_leading_ws(data: &[u8]) -> usize {
    let mut i = 0;
    while i < data.len() && data[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn skip_preamble_to_first_from(data: &[u8], mut pos: usize, keep_cr: bool) -> usize {
    while pos < data.len() {
        let start = pos;
        let end = data[pos..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|p| pos + p + 1)
            .unwrap_or(data.len());
        let mut line = data[start..end].to_vec();
        if !keep_cr {
            strip_crlf_end(&mut line);
        }
        if is_from_line(&line) {
            return start;
        }
        pos = end;
    }
    pos
}

fn strip_crlf_end(line: &mut Vec<u8>) {
    if line.len() >= 2 && line.ends_with(b"\n") && line[line.len() - 2] == b'\r' {
        line.truncate(line.len() - 2);
        line.push(b'\n');
    }
}

fn is_from_line(line: &[u8]) -> bool {
    if line.len() < 20 || !line.starts_with(b"From ") {
        return false;
    }
    if line.len() < 2 {
        return false;
    }
    let mut colon = line.len() - 2;
    let start = 5usize;
    loop {
        if colon < start {
            return false;
        }
        if line[colon] == b':' {
            break;
        }
        colon -= 1;
    }
    if colon < 4 {
        return false;
    }
    if !line[colon - 4].is_ascii_digit()
        || !line[colon - 2].is_ascii_digit()
        || !line[colon - 1].is_ascii_digit()
        || !line[colon + 1].is_ascii_digit()
        || !line[colon + 2].is_ascii_digit()
    {
        return false;
    }
    let tail = std::str::from_utf8(&line[colon + 3..]).unwrap_or("");
    let year_str = tail.trim_start();
    let year_digits: String = year_str
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let Ok(year) = year_digits.parse::<i64>() else {
        return false;
    };
    year > 90
}

fn is_gtfrom(buf: &[u8]) -> bool {
    let min = b">From ".len();
    if buf.len() < min {
        return false;
    }
    let ngt = buf.iter().take_while(|&&b| b == b'>').count();
    ngt > 0 && buf[ngt..].starts_with(b"From ")
}

fn split_one_message(
    data: &[u8],
    pos: &mut usize,
    out: &mut fs::File,
    allow_bare: bool,
    keep_cr: bool,
    mboxrd: bool,
) -> Result<bool> {
    let mut line = read_line_bytes(data, pos)?;
    if !keep_cr && line.len() > 1 && line.ends_with(b"\n") && line[line.len() - 2] == b'\r' {
        line.truncate(line.len() - 2);
        line.push(b'\n');
    }
    let is_bare = !is_from_line(&line);

    if is_bare && !allow_bare {
        bail!("corrupt mailbox");
    }

    loop {
        let mut write_line = line.clone();
        if !keep_cr
            && write_line.len() > 1
            && write_line.ends_with(b"\n")
            && write_line[write_line.len() - 2] == b'\r'
        {
            write_line.truncate(write_line.len() - 2);
            write_line.push(b'\n');
        }
        if mboxrd && is_gtfrom(&write_line) {
            write_line.remove(0);
        }
        out.write_all(&write_line)
            .context("writing split message")?;

        line = read_line_bytes(data, pos)?;
        if line.is_empty() {
            return Ok(true);
        }
        if !keep_cr && line.len() > 1 && line.ends_with(b"\n") && line[line.len() - 2] == b'\r' {
            line.truncate(line.len() - 2);
            line.push(b'\n');
        }
        if !is_bare && is_from_line(&line) {
            let rewind = line.len();
            *pos = (*pos).saturating_sub(rewind);
            return Ok(false);
        }
    }
}

fn read_line_bytes(data: &[u8], pos: &mut usize) -> Result<Vec<u8>> {
    if *pos >= data.len() {
        return Ok(Vec::new());
    }
    let start = *pos;
    while *pos < data.len() {
        let b = data[*pos];
        *pos += 1;
        if b == b'\n' {
            break;
        }
    }
    Ok(data[start..*pos].to_vec())
}
