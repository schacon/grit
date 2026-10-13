//! Terminal progress rendering for wire fetch/clone (`grit_lib::remote::fetch::Progress`).

use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use grit_lib::progress::{ProgressPhase, ProgressSink};
use grit_lib::remote::fetch::Progress;
use grit_lib::remote::sideband_lines::{RemoteLine, SidebandLines};
use terminal_size::{terminal_size, Width};

use crate::output::OutputMode;

const MIN_REDRAW: Duration = Duration::from_millis(100);

/// Human-mode progress on stderr when stderr is a TTY.
pub struct TerminalProgress {
    active: bool,
    phase: Option<ProgressPhase>,
    current: u64,
    total: Option<u64>,
    phase_started: Instant,
    last_draw: Instant,
    last_count: u64,
    remote_lines: SidebandLines,
}

impl TerminalProgress {
    /// Create a renderer; it emits nothing unless `mode` is human and stderr is a TTY.
    #[must_use]
    pub fn new(mode: OutputMode) -> Self {
        let now = Instant::now();
        Self {
            active: mode == OutputMode::Human && io::stderr().is_terminal(),
            phase: None,
            current: 0,
            total: None,
            phase_started: now,
            last_draw: now - MIN_REDRAW,
            last_count: 0,
            remote_lines: SidebandLines::new(),
        }
    }
}

impl Progress for TerminalProgress {
    fn message(&mut self, bytes: &[u8]) {
        if !self.active {
            return;
        }
        let mut lines = Vec::new();
        self.remote_lines.feed(bytes, |line| lines.push(line));
        for line in lines {
            self.render_remote(line);
        }
    }

    fn remote_sideband_progress(&self) -> bool {
        self.active
    }
}

impl ProgressSink for TerminalProgress {
    fn start(&mut self, phase: ProgressPhase, total: Option<u64>) {
        if !self.active {
            return;
        }
        if self.phase.is_some() {
            clear_progress_line();
        }
        self.phase = Some(phase);
        self.current = 0;
        self.total = total;
        self.phase_started = Instant::now();
        self.last_draw = Instant::now() - MIN_REDRAW;
        self.last_count = 0;
    }

    fn inc(&mut self, units: u64) {
        if !self.active {
            return;
        }
        self.current = self.current.saturating_add(units);
        self.maybe_redraw(false);
    }

    fn set(&mut self, current: u64) {
        if !self.active {
            return;
        }
        self.current = current;
        self.maybe_redraw(false);
    }

    fn finish(&mut self) {
        if !self.active {
            return;
        }
        let Some(phase) = self.phase.take() else {
            return;
        };
        let width = terminal_width();
        let rate = rate_bytes_per_sec(self.current, self.phase_started.elapsed());
        let line = render_line(
            ProgressLineState {
                phase,
                current: self.current,
                total: self.total,
                rate_bytes_per_sec: rate,
                done: true,
            },
            width,
        );
        let _ = write_finish_line(&mut io::stderr(), &line, width);
        self.current = 0;
        self.total = None;
    }
}

impl TerminalProgress {
    fn render_remote(&mut self, line: RemoteLine) {
        let prefix = match &line {
            RemoteLine::Update(text) | RemoteLine::Line(text) => {
                if text.is_empty() {
                    return;
                }
                format!("remote: {text}")
            }
        };
        if self.phase.is_some() {
            clear_progress_line();
        }
        match line {
            RemoteLine::Update(_) => {
                let _ = write!(io::stderr(), "\r{prefix}");
                let _ = io::stderr().flush();
            }
            RemoteLine::Line(_) => {
                let width = terminal_width();
                let _ = write_finish_line(&mut io::stderr(), &prefix, width);
            }
        }
    }

    fn maybe_redraw(&mut self, force: bool) {
        let Some(phase) = self.phase else {
            return;
        };
        let now = Instant::now();
        if !force && now.duration_since(self.last_draw) < MIN_REDRAW {
            return;
        }
        let elapsed = now.duration_since(self.phase_started);
        let rate = rate_bytes_per_sec(self.current, elapsed);
        let width = terminal_width();
        let line = render_line(
            ProgressLineState {
                phase,
                current: self.current,
                total: self.total,
                rate_bytes_per_sec: rate,
                done: false,
            },
            width,
        );
        let _ = write!(io::stderr(), "\r{line}");
        let _ = io::stderr().flush();
        self.last_draw = now;
        self.last_count = self.current;
    }
}

fn terminal_width() -> usize {
    terminal_size()
        .map(|(Width(w), _)| w as usize)
        .unwrap_or(80)
        .max(20)
}

fn clear_progress_line() {
    let width = terminal_width();
    let _ = write_clear_line(&mut io::stderr(), width);
}

/// Return to column 0 and wipe the current terminal row (no newline).
fn write_clear_line(w: &mut impl Write, width: usize) -> io::Result<()> {
    write!(w, "\r{}\r", " ".repeat(width))
}

/// Replace an in-place progress redraw with a final line (includes newline).
fn write_finish_line(w: &mut impl Write, line: &str, width: usize) -> io::Result<()> {
    write_clear_line(w, width)?;
    write!(w, "{line}")?;
    writeln!(w)
}

/// Inputs for [`render_line`].
#[derive(Debug, Clone, Copy)]
pub struct ProgressLineState {
    pub phase: ProgressPhase,
    pub current: u64,
    pub total: Option<u64>,
    pub rate_bytes_per_sec: f64,
    pub done: bool,
}

/// Format a byte count for display (binary units).
#[must_use]
pub fn format_bytes(n: u64) -> String {
    const KIB: f64 = 1024.0;
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if n == 0 {
        return "0 B".to_owned();
    }
    let mut value = n as f64;
    let mut unit = 0usize;
    while value >= KIB && unit + 1 < UNITS.len() {
        value /= KIB;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

/// Format a transfer rate (bytes per second).
#[must_use]
pub fn format_rate(bytes_per_sec: f64) -> String {
    if !bytes_per_sec.is_finite() || bytes_per_sec <= 0.0 {
        return "0 B/s".to_owned();
    }
    format!("{}/s", format_bytes(bytes_per_sec.round() as u64))
}

fn phase_label(phase: ProgressPhase) -> &'static str {
    match phase {
        ProgressPhase::ReceivingObjects => "Receiving objects",
        ProgressPhase::IndexingObjects => "Indexing objects",
        ProgressPhase::ResolvingDeltas => "Resolving deltas",
        ProgressPhase::CopyingObjects => "Copying objects",
        ProgressPhase::CheckingOutFiles => "Checking out files",
        _ => "Working",
    }
}

fn rate_bytes_per_sec(bytes: u64, elapsed: Duration) -> f64 {
    let secs = elapsed.as_secs_f64();
    if secs <= 0.0 {
        return 0.0;
    }
    bytes as f64 / secs
}

/// Build the progress line (without the leading `\r`).
#[must_use]
pub fn render_line(state: ProgressLineState, width: usize) -> String {
    let label = phase_label(state.phase);
    let mut line = match state.total {
        Some(total) if total > 0 => {
            let pct = (state.current.saturating_mul(100) / total).min(100);
            format!(
                "{label}: {}/{} ({}%), {}, {}",
                state.current,
                total,
                pct,
                format_bytes(state.current),
                format_rate(state.rate_bytes_per_sec)
            )
        }
        _ => format!(
            "{label}: {}, {}",
            format_bytes(state.current),
            format_rate(state.rate_bytes_per_sec)
        ),
    };
    if state.done {
        line.push_str(", done.");
    }
    if line.len() > width {
        line.truncate(width);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.50 KiB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.00 MiB");
    }

    #[test]
    fn formats_rate() {
        assert_eq!(format_rate(0.0), "0 B/s");
        assert_eq!(format_rate(1024.0), "1.00 KiB/s");
    }

    #[test]
    fn renders_known_total() {
        let line = render_line(
            ProgressLineState {
                phase: ProgressPhase::ReceivingObjects,
                current: 50,
                total: Some(100),
                rate_bytes_per_sec: 2048.0,
                done: false,
            },
            120,
        );
        assert!(line.contains("Receiving objects: 50/100 (50%)"));
        assert!(line.contains("50 B"));
        assert!(line.contains("2.00 KiB/s"));
    }

    #[test]
    fn renders_unknown_total() {
        let line = render_line(
            ProgressLineState {
                phase: ProgressPhase::ReceivingObjects,
                current: 4096,
                total: None,
                rate_bytes_per_sec: 8192.0,
                done: false,
            },
            120,
        );
        assert_eq!(line, "Receiving objects: 4.00 KiB, 8.00 KiB/s");
    }

    #[test]
    fn finish_replaces_prior_redraw_on_stderr() {
        let width = 120;
        let mid = render_line(
            ProgressLineState {
                phase: ProgressPhase::ReceivingObjects,
                current: 8192,
                total: None,
                rate_bytes_per_sec: 13_000_000_000.0,
                done: false,
            },
            width,
        );
        let final_line = render_line(
            ProgressLineState {
                phase: ProgressPhase::ReceivingObjects,
                current: 57_000,
                total: None,
                rate_bytes_per_sec: 1_120_000.0,
                done: true,
            },
            width,
        );
        let mut stream = Vec::new();
        write!(stream, "\r{mid}").expect("redraw");
        write_finish_line(&mut stream, &final_line, width).expect("finish");

        let text = String::from_utf8(stream).expect("utf8");
        assert!(
            !text.contains("/sReceiving objects"),
            "finish must not append after an in-place redraw: {text:?}"
        );
        assert!(
            text.ends_with(", done.\n"),
            "expected final newline after done line: {text:?}"
        );
        let after_last_cr = text.rsplit('\r').next().expect("cr");
        assert!(
            after_last_cr.ends_with(", done.\n"),
            "visible tail must be the done line only: {after_last_cr:?}"
        );
    }

    #[test]
    fn truncates_to_width() {
        let line = render_line(
            ProgressLineState {
                phase: ProgressPhase::ReceivingObjects,
                current: 1_000_000,
                total: None,
                rate_bytes_per_sec: 1_000_000.0,
                done: false,
            },
            20,
        );
        assert_eq!(line.len(), 20);
    }
}
