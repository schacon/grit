#!/usr/bin/env python3
"""Generate docs/timeline.html: hourly commit chart and full ``git log --all`` listing."""

from __future__ import annotations

import html
import os
import subprocess
from collections import Counter
from datetime import date, datetime, timedelta, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "docs" / "timeline.html"

# Field separator (unlikely in subjects); record ends with RS.
FS = "\x1f"
RS = "\x1e"


def github_commit_url(sha: str) -> str | None:
    """Return an https://github.com/.../commit/SHA URL if ``origin`` is GitHub."""
    if not sha or sha == "unknown":
        return None
    try:
        raw = subprocess.check_output(
            ["git", "config", "--get", "remote.origin.url"],
            cwd=REPO,
            text=True,
        ).strip()
    except Exception:
        return None
    raw = raw.rstrip("/")
    if raw.endswith(".git"):
        raw = raw[:-4]
    owner: str | None = None
    repo: str | None = None
    if raw.startswith("git@"):
        host_and_rest = raw.partition("@")[2]
        domain, _, path = host_and_rest.partition(":")
        if domain != "github.com" or "/" not in path:
            return None
        owner, repo = path.split("/", 1)
    elif "github.com/" in raw:
        after = raw.split("github.com/", 1)[1]
        segs = after.strip("/").split("/")
        if len(segs) >= 2:
            owner, repo = segs[0], segs[1]
    if not owner or not repo:
        return None
    repo = repo.removesuffix(".git")
    return f"https://github.com/{owner}/{repo}/commit/{sha}"


def git_head_sha() -> str:
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=REPO, text=True
        ).strip()
    except Exception:
        return ""


def generated_time_element(now: datetime) -> str:
    gen_time = now.strftime("%Y-%m-%d %H:%M UTC")
    iso = now.replace(microsecond=0).isoformat()
    return (
        f'<time datetime="{html.escape(iso)}" class="gen-time" title="{html.escape(gen_time)}">'
        f"{html.escape(gen_time)}</time>"
    )


def parse_git_iso_date(iso: str) -> datetime | None:
    """Parse ``%aI`` timestamps from git log."""
    try:
        s = iso.replace("Z", "+00:00")
        return datetime.fromisoformat(s)
    except ValueError:
        return None


def to_utc(dt: datetime) -> datetime:
    """Normalize to UTC (naive datetimes are treated as UTC)."""
    if dt.tzinfo is None:
        return dt.replace(tzinfo=timezone.utc)
    return dt.astimezone(timezone.utc)


def floor_hour_utc(dt: datetime) -> datetime:
    """Start of the UTC hour containing ``dt``."""
    u = to_utc(dt)
    return u.replace(minute=0, second=0, microsecond=0)


def history_start_date(commits: list[tuple[str, str, str, str, str]]) -> date:
    """Calendar day of the earliest author timestamp in ``commits`` (for captions)."""
    parsed = [parse_git_iso_date(c[3]) for c in commits]
    valid = [d for d in parsed if d is not None]
    if not valid:
        return date.today()
    return min(d.date() for d in valid)


def short_hour_axis_label(cur: datetime, prev: datetime | None) -> str:
    """Compact UTC label: include date when the calendar day changes."""
    cur_f = floor_hour_utc(cur)
    if prev is None:
        return cur_f.strftime("%b %d %H:%M")
    prev_f = floor_hour_utc(prev)
    if cur_f.date() != prev_f.date():
        return cur_f.strftime("%b %d %H:%M")
    return cur_f.strftime("%H:%M")


def hourly_commit_histogram(
    commits: list[tuple[str, str, str, str, str]],
) -> tuple[list[tuple[str, int, str]], int]:
    """
    One bar per UTC hour from the first commit hour through the last.

    Empty hours between the first and last commit are filled with zero.
    """
    dates: list[datetime] = []
    for *_, iso_date, _ in commits:
        parsed = parse_git_iso_date(iso_date)
        if parsed is not None:
            dates.append(parsed)
    if not dates:
        return [], 0
    lo = floor_hour_utc(min(dates))
    hi = floor_hour_utc(max(dates))
    counts: Counter[str] = Counter()
    for dt in dates:
        key = floor_hour_utc(dt).isoformat()
        counts[key] += 1
    rows: list[tuple[str, int, str]] = []
    cur = lo
    prev: datetime | None = None
    while cur <= hi:
        key = cur.isoformat()
        c = counts.get(key, 0)
        lbl = short_hour_axis_label(cur, prev)
        rows.append((key, c, lbl))
        prev = cur
        cur += timedelta(hours=1)
    peak = max((r[1] for r in rows), default=0)
    return rows, peak


def dt_from_iso_key(key: str) -> datetime:
    """Parse an ISO key produced by ``floor_hour_utc(...).isoformat()``."""
    return datetime.fromisoformat(key.replace("Z", "+00:00"))


def count_axis_ticks(peak: int) -> list[int]:
    """
    Descending tick values for the Y axis (top = highest, bottom = 0).

    Uses up to five evenly spaced integer ticks, deduplicated.
    """
    if peak <= 0:
        return [0]
    raw = [round(peak * i / 4) for i in range(5)]
    uniq = sorted(set(raw), reverse=True)
    return uniq


def chart_label_stride(num_bars: int) -> int:
    """Show every Nth x-axis label so dense charts stay legible."""
    if num_bars <= 24:
        return 1
    if num_bars <= 72:
        return 2
    if num_bars <= 168:
        return 4
    return max(1, (num_bars + 23) // 24)


def render_chart_html(
    hist_rows: list[tuple[str, int, str]], peak: int, first_commit_day: date
) -> str:
    """HTML for the hourly commit histogram (CSS column chart)."""
    if not hist_rows:
        return ""
    stride = chart_label_stride(len(hist_rows))
    y_ticks = count_axis_ticks(peak)
    y_tick_html = "".join(
        f'<span class="chart-y-tick">{html.escape(str(t))}</span>' for t in y_ticks
    )
    grid_lines: list[str] = []
    for i in range(5):
        pct = i * 25
        grid_lines.append(
            f'<span class="chart-grid-line" style="top:{pct}%"></span>'
        )
    grid_html = "".join(grid_lines)

    parts: list[str] = []
    prev_day: date | None = None
    for i, (key, count, label) in enumerate(hist_rows):
        cur_dt = dt_from_iso_key(key)
        cur_day = to_utc(cur_dt).date()
        is_new_day = prev_day is not None and cur_day != prev_day
        prev_day = cur_day
        day_cls = " chart-col-day-sep" if is_new_day else ""
        pct = (100.0 * count / peak) if peak else 0.0
        show_lbl = i % stride == 0 or i == len(hist_rows) - 1
        title = f"{key} UTC — {count} commit" + ("" if count == 1 else "s")
        lbl = (
            f'<span class="chart-x">{html.escape(label)}</span>'
            if show_lbl
            else '<span class="chart-x chart-x-skip" aria-hidden="true">·</span>'
        )
        parts.append(
            f'<div class="chart-col{day_cls}">'
            '<div class="chart-bar-wrap">'
            f'<div class="chart-bar" style="height:{pct:.3f}%" title="{html.escape(title)}"></div>'
            "</div>"
            f"{lbl}"
            "</div>"
        )
    peak_s = f"{peak:,}" if peak else "0"
    ncols = len(hist_rows)
    start_s = first_commit_day.isoformat()
    return (
        '<section class="chart-section" aria-label="Commits over time">'
        "<h2>Commits over time</h2>"
        f'<p class="chart-caption">One bar per hour (UTC) · full <code>git log --all</code> · '
        f'earliest commit day <strong>{html.escape(start_s)}</strong> · peak '
        f'<strong>{html.escape(peak_s)}</strong> commits/hour</p>'
        '<div class="chart-chart-wrap">'
        '<div class="chart-y-axis" role="presentation" aria-label="Commit count">'
        f'<div class="chart-y-axis-inner">{y_tick_html}</div>'
        '<div class="chart-y-axis-title">Commits</div>'
        "</div>"
        '<div class="chart-scroll chart-scroll-plot">'
        '<div class="chart-surface">'
        f'<div class="chart-grid-lines">{grid_html}</div>'
        f'<div class="chart-bars chart-bars-hourly" style="--chart-cols:{ncols}">'
        f'{"".join(parts)}'
        "</div></div></div></div></section>"
    )


def load_commits() -> list[tuple[str, str, str, str, str]]:
    """Return (full_sha, short_sha, author, iso_date, subject) newest first from ``git log --all``."""
    fmt = f"%H{FS}%h{FS}%an{FS}%aI{FS}%s{RS}"
    raw = subprocess.check_output(
        [
            "git",
            "-c",
            "log.showSignature=false",
            "log",
            "--all",
            f"--pretty=format:{fmt}",
        ],
        cwd=REPO,
    )
    text = raw.decode("utf-8", errors="replace")
    rows: list[tuple[str, str, str, str, str]] = []
    for rec in text.split(RS):
        rec = rec.strip()
        if not rec:
            continue
        parts = rec.split(FS, 4)
        if len(parts) != 5:
            continue
        rows.append((parts[0], parts[1], parts[2], parts[3], parts[4]))
    return rows


def main() -> None:
    os.chdir(REPO)
    commits = load_commits()
    first_day = history_start_date(commits)
    now = datetime.now(timezone.utc)
    head = git_head_sha()
    short = head[:7] if len(head) >= 7 else (head or "unknown")
    gh = github_commit_url(head) if head else None
    commit_link = (
        f'<a href="{html.escape(gh)}" style="color:#58a6ff">{html.escape(short)}</a>'
        if gh
        else html.escape(short)
    )

    hist_rows, hist_peak = hourly_commit_histogram(commits)
    chart_html = render_chart_html(hist_rows, hist_peak, first_day)

    rows_html: list[str] = []
    for full, short_sha, author, iso_date, subject in commits:
        url = github_commit_url(full)
        sha_cell = (
            f'<a href="{html.escape(url)}">{html.escape(short_sha)}</a>'
            if url
            else html.escape(short_sha)
        )
        rows_html.append(
            "<tr>"
            f'<td class="sha">{sha_cell}</td>'
            f'<td class="date" title="{html.escape(iso_date)}">{html.escape(iso_date[:10])}</td>'
            f'<td class="author">{html.escape(author)}</td>'
            f'<td class="subj">{html.escape(subject)}</td>'
            "</tr>"
        )

    body = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Grit — commit timeline</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
  background: #0d1117;
  color: #e6edf3;
  padding: 2rem;
  max-width: 1200px;
  margin: 0 auto;
}}
h1 {{ font-size: 1.75rem; margin-bottom: 0.25rem; color: #f0f6fc; }}
.sub {{ color: #7d8590; font-size: 0.9rem; margin-bottom: 1.25rem; }}
.sub time.gen-time {{ color: inherit; }}
.sub a {{ color: #58a6ff; }}
.count {{ color: #8b949e; font-size: 0.85rem; margin-bottom: 1rem; }}
table {{ width: 100%; border-collapse: collapse; font-size: 0.85rem; }}
th, td {{ text-align: left; padding: 0.35rem 0.5rem; border-bottom: 1px solid #21262d; vertical-align: top; }}
th {{ color: #7d8590; font-weight: 600; font-size: 0.72rem; text-transform: uppercase; letter-spacing: 0.05em; position: sticky; top: 0; background: #0d1117; z-index: 1; }}
.sha {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; white-space: nowrap; width: 1%; }}
.sha a {{ color: #58a6ff; text-decoration: none; }}
.sha a:hover {{ text-decoration: underline; }}
.date {{ color: #8b949e; white-space: nowrap; width: 1%; }}
.author {{ color: #c9d1d9; max-width: 12rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
.subj {{ color: #e6edf3; word-break: break-word; }}
.chart-section {{ margin-bottom: 2.5rem; }}
.chart-section h2 {{ font-size: 1.15rem; color: #f0f6fc; margin-bottom: 0.35rem; font-weight: 600; }}
.chart-caption {{ color: #8b949e; font-size: 0.8rem; margin-bottom: 1rem; line-height: 1.45; }}
.chart-caption strong {{ color: #c9d1d9; font-weight: 600; }}
.chart-chart-wrap {{
  display: flex;
  align-items: flex-start;
  gap: 0;
  margin-top: 0.35rem;
}}
.chart-y-axis {{
  flex-shrink: 0;
  width: 3rem;
  padding-right: 0.45rem;
  border-right: 1px solid #30363d;
}}
.chart-y-axis-inner {{
  height: 200px;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  align-items: flex-end;
  font-size: 0.68rem;
  color: #7d8590;
  font-variant-numeric: tabular-nums;
  line-height: 1;
}}
.chart-y-tick {{
  display: block;
}}
.chart-y-axis-title {{
  margin-top: 0.5rem;
  font-size: 0.62rem;
  color: #484f58;
  text-align: right;
  line-height: 1.2;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}}
.chart-scroll {{
  overflow-x: auto;
  padding: 0.5rem 0 2.75rem;
  margin: 0 -0.25rem;
  scrollbar-color: #30363d #161b22;
}}
.chart-scroll-plot {{
  flex: 1;
  min-width: 0;
  margin: 0;
  padding-top: 0;
}}
.chart-surface {{
  position: relative;
  min-height: 200px;
}}
.chart-grid-lines {{
  position: absolute;
  left: 0;
  right: 0;
  top: 0;
  height: 200px;
  pointer-events: none;
  z-index: 0;
}}
.chart-grid-line {{
  position: absolute;
  left: 0;
  right: 0;
  height: 1px;
  background: #21262d;
  margin-top: -0.5px;
}}
.chart-bars {{
  display: flex;
  align-items: flex-end;
  gap: 2px;
  min-height: 200px;
  height: auto;
  min-width: max(100%, calc(var(--chart-cols, 1) * 7px));
  padding: 0 6px;
  position: relative;
  z-index: 1;
}}
.chart-col {{
  flex: 1 0 6px;
  min-width: 5px;
  max-width: 28px;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  align-items: center;
}}
.chart-bars-hourly {{
  min-width: max(100%, calc(var(--chart-cols, 1) * 3px));
  gap: 1px;
}}
.chart-bars-hourly .chart-col {{
  max-width: 12px;
  min-width: 2px;
}}
.chart-col-day-sep {{
  border-left: 2px solid #484f58;
  margin-left: 1px;
  padding-left: 1px;
}}
.chart-bar-wrap {{
  flex: none;
  height: 200px;
  width: 100%;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  min-height: 0;
}}
.chart-bar {{
  width: 100%;
  min-height: 2px;
  border-radius: 3px 3px 1px 1px;
  background: linear-gradient(180deg, #58a6ff 0%, #388bfd 45%, #1f6feb 100%);
  box-shadow: 0 0 14px rgba(88, 166, 255, 0.12);
  transition: filter 0.12s ease, box-shadow 0.12s ease;
}}
.chart-col:hover .chart-bar {{
  filter: brightness(1.12);
  box-shadow: 0 0 18px rgba(88, 166, 255, 0.22);
}}
.chart-x {{
  flex-shrink: 0;
  font-size: 0.58rem;
  color: #7d8590;
  margin-top: 0.35rem;
  line-height: 1.05;
  text-align: center;
  max-width: 140%;
  white-space: nowrap;
  transform: rotate(-42deg);
  transform-origin: top center;
  height: 2.4rem;
}}
.chart-x-skip {{ visibility: hidden; }}
</style>
</head>
<body>
<h1>Commit timeline</h1>
<p class="sub">Generated {generated_time_element(now)} · {commit_link} · <a href="index.html">Dashboard</a> · <a href="testfiles.html">All test files</a></p>
<p class="count">{len(commits):,} commits from <code>git log --all</code> (earliest day {html.escape(first_day.isoformat())})</p>
{chart_html}
<table>
<thead>
<tr><th>Commit</th><th>Date</th><th>Author</th><th>Subject</th></tr>
</thead>
<tbody>
{"\n".join(rows_html)}
</tbody>
</table>
<script>
(function() {{
  document.querySelectorAll('time.gen-time').forEach(function(el) {{
    var dt = el.getAttribute('datetime');
    if (!dt) return;
    var d = new Date(dt);
    if (isNaN(d.getTime())) return;
    var rtf = new Intl.RelativeTimeFormat('en', {{ numeric: 'auto' }});
    var now = new Date();
    var diffSec = (d - now) / 1000;
    var abs = Math.abs(diffSec);
    var v;
    var unit;
    if (abs < 60) {{ v = Math.round(diffSec); unit = 'second'; }}
    else if (abs < 3600) {{ v = Math.round(diffSec / 60); unit = 'minute'; }}
    else if (abs < 86400) {{ v = Math.round(diffSec / 3600); unit = 'hour'; }}
    else if (abs < 604800) {{ v = Math.round(diffSec / 86400); unit = 'day'; }}
    else if (abs < 2629800) {{ v = Math.round(diffSec / 604800); unit = 'week'; }}
    else if (abs < 31536000) {{ v = Math.round(diffSec / 2629800); unit = 'month'; }}
    else {{ v = Math.round(diffSec / 31536000); unit = 'year'; }}
    el.textContent = rtf.format(v, unit);
  }});
}})();
</script>
</body>
</html>
"""
    OUT.write_text(body, encoding="utf-8")
    print(f"Wrote {OUT} ({len(commits)} commits)")


if __name__ == "__main__":
    main()
