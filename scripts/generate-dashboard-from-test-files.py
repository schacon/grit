#!/usr/bin/env python3
"""Generate docs/index.html and docs/testfiles.html from data/test-files.csv."""

from __future__ import annotations

import csv
import html
import os
import subprocess
import urllib.parse
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DATA = REPO / "data" / "test-files.csv"
OUT_INDEX = REPO / "docs" / "index.html"
OUT_FILES = REPO / "docs" / "testfiles.html"

GROUP_DESC: dict[str, str] = {
    "t0": "Basic / setup",
    "t1": "Plumbing: read-tree, cat-file, refs",
    "t2": "Checkout / index",
    "t3": "ls-files, merge, cherry-pick, rm, add, mv",
    "t4": "Diff",
    "t5": "Pack / fetch / push / clone",
    "t6": "Rev-list, rev-parse, merge-base, for-each-ref",
    "t7": "Porcelain: commit, status, tag, branch, reset",
    "t8": "Git-p4 / misc",
    "t9": "Contrib / completion",
}


def git_short_sha() -> str:
    try:
        return (
            subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=REPO, text=True
            ).strip()[:7]
        )
    except Exception:
        return "unknown"


def load_rows() -> list[dict[str, str]]:
    if not DATA.exists():
        return []
    rows: list[dict[str, str]] = []
    with DATA.open(newline="", encoding="utf-8") as f:
        reader = csv.DictReader(f, delimiter="\t")
        for row in reader:
            rows.append(dict(row))
    return rows


def pct(n: int, d: int) -> float:
    return round(100.0 * n / d, 1) if d > 0 else 0.0


def generate_index(rows: list[dict[str, str]]) -> str:
    in_scope = [r for r in rows if r.get("in_scope", "yes").strip().lower() != "skip"]
    skipped = [r for r in rows if r.get("in_scope", "yes").strip().lower() == "skip"]

    total_tests = 0
    total_pass = 0
    full_files = 0
    for r in in_scope:
        try:
            tt = int(r.get("tests_total") or 0)
            pl = int(r.get("passed_last") or 0)
        except ValueError:
            continue
        total_tests += tt
        total_pass += pl
        fp = (r.get("fully_passing") or "").strip().lower() == "true"
        if fp and tt > 0:
            full_files += 1

    file_count = len(in_scope)
    pass_rate = pct(total_pass, total_tests)

    now = datetime.now(timezone.utc)
    gen_time = now.strftime("%Y-%m-%d %H:%M UTC")
    sha = git_short_sha()

    groups: dict[str, dict[str, int]] = {}
    for r in in_scope:
        g = r.get("group") or "t?"
        if g not in groups:
            groups[g] = {"tests": 0, "pass": 0, "files": 0, "full": 0}
        try:
            tt = int(r.get("tests_total") or 0)
            pl = int(r.get("passed_last") or 0)
        except ValueError:
            tt, pl = 0, 0
        groups[g]["tests"] += tt
        groups[g]["pass"] += pl
        groups[g]["files"] += 1
        if (r.get("fully_passing") or "").lower() == "true" and tt > 0:
            groups[g]["full"] += 1

    order = sorted(groups.keys(), key=lambda x: (len(x), x))

    group_html = ""
    for g in order:
        st = groups[g]
        desc = GROUP_DESC.get(g, "Tests")
        ttot, tpass = st["tests"], st["pass"]
        pc = pct(tpass, ttot)
        q = urllib.parse.urlencode({"group": g})
        href = f"testfiles.html?{q}"
        group_html += f"""
    <a class="group-card" href="{html.escape(href)}">
      <div class="group-head">
        <span class="group-id">{html.escape(g)}</span>
        <span class="group-meta">{st["full"]}/{st["files"]} files · {tpass:,}/{ttot:,} tests</span>
      </div>
      <p class="group-desc">{html.escape(desc)}</p>
      <div class="bar-bg"><div class="bar-fg" style="width:{pc}%"></div></div>
      <div class="group-pct">{pc}%</div>
    </a>"""

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Grit test dashboard</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
  background: #0d1117;
  color: #e6edf3;
  padding: 2rem;
  max-width: 960px;
  margin: 0 auto;
}}
h1 {{ font-size: 1.75rem; margin-bottom: 0.25rem; color: #f0f6fc; }}
.sub {{ color: #7d8590; font-size: 0.9rem; margin-bottom: 1.75rem; }}
.cards {{
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 1rem;
  margin-bottom: 2rem;
}}
.card {{
  background: #161b22;
  border: 1px solid #30363d;
  border-radius: 8px;
  padding: 1rem;
  text-align: center;
}}
.card .n {{ font-size: 1.65rem; font-weight: 700; color: #f0f6fc; }}
.card .lbl {{ font-size: 0.72rem; color: #7d8590; margin-top: 0.35rem; text-transform: uppercase; letter-spacing: 0.04em; }}
.card.accent .n {{ color: #3fb950; }}
h2 {{ font-size: 1.1rem; margin-bottom: 1rem; color: #f0f6fc; }}
.group-card {{
  display: block;
  background: #161b22;
  border: 1px solid #30363d;
  border-radius: 8px;
  padding: 1rem 1.1rem;
  margin-bottom: 0.75rem;
  text-decoration: none;
  color: inherit;
  transition: border-color 0.15s;
}}
.group-card:hover {{ border-color: #58a6ff; }}
.group-head {{ display: flex; justify-content: space-between; align-items: baseline; flex-wrap: wrap; gap: 0.5rem; }}
.group-id {{ font-weight: 700; color: #58a6ff; font-size: 1rem; }}
.group-meta {{ font-size: 0.78rem; color: #7d8590; }}
.group-desc {{ font-size: 0.85rem; color: #8b949e; margin: 0.5rem 0 0.75rem; }}
.bar-bg {{ background: #21262d; border-radius: 6px; height: 10px; overflow: hidden; border: 1px solid #30363d; }}
.bar-fg {{ height: 100%; background: linear-gradient(90deg, #238636, #2ea043); border-radius: 6px 0 0 6px; }}
.group-pct {{ font-size: 0.8rem; color: #7d8590; margin-top: 0.35rem; text-align: right; }}
</style>
</head>
<body>
<h1>Grit test dashboard</h1>
<p class="sub">Generated {html.escape(gen_time)} · {html.escape(sha)} · <a href="testfiles.html" style="color:#58a6ff">All test files</a></p>

<div class="cards">
  <div class="card"><div class="n">{file_count:,}</div><div class="lbl">Test files (in scope)</div></div>
  <div class="card accent"><div class="n">{full_files:,}</div><div class="lbl">Files fully passing</div></div>
  <div class="card"><div class="n">{total_tests:,}</div><div class="lbl">Tests (total)</div></div>
  <div class="card accent"><div class="n">{total_pass:,}</div><div class="lbl">Tests passed</div></div>
  <div class="card"><div class="n">{pass_rate}%</div><div class="lbl">Tests passing</div></div>
  <div class="card"><div class="n">{len(skipped):,}</div><div class="lbl">Manually skipped files</div></div>
</div>

<h2>Groups</h2>
<p class="sub" style="margin-bottom:1rem">Click a group for per-file detail. Counts exclude manually skipped files.</p>
{group_html}
</body>
</html>
"""


def generate_testfiles(rows: list[dict[str, str]]) -> str:
    now = datetime.now(timezone.utc)
    gen_time = now.strftime("%Y-%m-%d %H:%M UTC")
    sha = git_short_sha()

    in_scope = [r for r in rows if r.get("in_scope", "yes").strip().lower() != "skip"]
    skipped_rows = [r for r in rows if r.get("in_scope", "yes").strip().lower() == "skip"]

    groups_order = sorted({r.get("group") or "t?" for r in rows}, key=lambda x: (len(x), x))

    table_rows = ""
    for r in sorted(rows, key=lambda x: x.get("file", "")):
        base = r.get("file", "")
        g = r.get("group", "")
        iscope = r.get("in_scope", "yes").strip().lower()
        is_skip = iscope == "skip"
        try:
            tt = int(r.get("tests_total") or 0)
            pl = int(r.get("passed_last") or 0)
            fl = int(r.get("failing") or 0)
        except ValueError:
            tt, pl, fl = 0, 0, 0
        fp = (r.get("fully_passing") or "").strip().lower() == "true"
        st = r.get("status", "")
        ef = r.get("expect_failure", "0")
        skip_badge = (
            '<span class="badge skip">skipped</span>' if is_skip else ""
        )
        fp_badge = (
            '<span class="badge ok">full pass</span>'
            if fp and tt > 0 and not is_skip
            else ""
        )
        pc = pct(pl, tt) if tt > 0 else 0.0
        row_cls = "row-skip" if is_skip else ""
        table_rows += f"""
<tr class="{row_cls}" data-group="{html.escape(g)}">
  <td class="mono">{html.escape(base)}</td>
  <td>{html.escape(g)}</td>
  <td>{skip_badge}{fp_badge}</td>
  <td class="right">{tt if tt or not is_skip else "—"}</td>
  <td class="right">{pl if not is_skip else "—"}</td>
  <td class="right">{"—" if is_skip else html.escape(st)}</td>
  <td class="bar"><div class="bar-bg"><div class="bar-fg" style="width:{pc if not is_skip else 0}%"></div></div></td>
  <td class="right small">{html.escape(ef)}</td>
</tr>"""

    options = '<option value="">All groups</option>\n'
    for g in groups_order:
        lab = f"{g} — {GROUP_DESC.get(g, '')}"
        options += f'  <option value="{html.escape(g)}">{html.escape(lab)}</option>\n'

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Grit test files</title>
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
h1 {{ font-size: 1.5rem; margin-bottom: 0.25rem; }}
.sub {{ color: #7d8590; margin-bottom: 1.25rem; font-size: 0.9rem; }}
a {{ color: #58a6ff; text-decoration: none; }}
a:hover {{ text-decoration: underline; }}
.toolbar {{ display: flex; flex-wrap: wrap; gap: 0.75rem; align-items: center; margin-bottom: 1rem; }}
select, input {{
  background: #161b22;
  border: 1px solid #30363d;
  border-radius: 6px;
  color: #e6edf3;
  padding: 0.45rem 0.65rem;
  font-size: 0.85rem;
}}
input {{ min-width: 220px; }}
table {{ width: 100%; border-collapse: collapse; }}
th {{
  text-align: left;
  padding: 0.5rem 0.5rem;
  font-size: 0.72rem;
  color: #7d8590;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  border-bottom: 1px solid #21262d;
}}
td {{ padding: 0.45rem 0.5rem; font-size: 0.84rem; border-bottom: 1px solid #161b22; }}
tr:hover td {{ background: #161b22; }}
tr.row-skip td {{ opacity: 0.65; }}
.mono {{ font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 0.82rem; }}
.right {{ text-align: right; }}
.small {{ font-size: 0.78rem; color: #7d8590; }}
.bar {{ width: 100px; }}
.bar-bg {{ background: #21262d; border-radius: 4px; height: 8px; overflow: hidden; }}
.bar-fg {{ height: 100%; background: linear-gradient(90deg, #238636, #2ea043); border-radius: 4px 0 0 4px; }}
.badge {{ font-size: 0.72rem; padding: 0.15rem 0.4rem; border-radius: 4px; margin-right: 0.35rem; }}
.badge.skip {{ background: #3d2f00; color: #d29922; border: 1px solid #6e4b0a; }}
.badge.ok {{ background: #0d2818; color: #3fb950; border: 1px solid #238636; }}
.hint {{ color: #7d8590; font-size: 0.82rem; margin-top: 1rem; }}
</style>
</head>
<body>
<h1>Test files</h1>
<p class="sub"><a href="index.html">Dashboard</a> · {html.escape(gen_time)} · {html.escape(sha)}</p>

<div class="toolbar">
  <label for="groupSel">Group</label>
  <select id="groupSel" aria-label="Filter by group">{options}</select>
  <input type="search" id="search" placeholder="Filter by file name…" aria-label="Search">
  <span id="count" class="sub"></span>
</div>

<table>
<thead>
<tr>
  <th>File</th>
  <th>Group</th>
  <th>Scope</th>
  <th class="right">Tests</th>
  <th class="right">Passed</th>
  <th>Status</th>
  <th>Progress</th>
  <th class="right">expect_failure</th>
</tr>
</thead>
<tbody id="tbody">
{table_rows}
</tbody>
</table>
<p class="hint">Manually skipped files are marked and excluded from dashboard totals on the main page. Rows with <code>expect_failure</code> count known-breakage stubs in the harness.</p>

<script>
(function() {{
  const params = new URLSearchParams(window.location.search);
  const initial = params.get('group') || '';
  const sel = document.getElementById('groupSel');
  const search = document.getElementById('search');
  sel.value = initial;

  function apply() {{
    const g = sel.value;
    const q = (search.value || '').toLowerCase();
    const rows = document.querySelectorAll('#tbody tr');
    let n = 0;
    rows.forEach(row => {{
      const rg = row.dataset.group || '';
      const file = row.cells[0].textContent.toLowerCase();
      const okG = !g || rg === g;
      const okQ = !q || file.includes(q);
      const show = okG && okQ;
      row.style.display = show ? '' : 'none';
      if (show) n++;
    }});
    document.getElementById('count').textContent = n + ' files shown';
    const u = new URL(window.location.href);
    if (g) u.searchParams.set('group', g); else u.searchParams.delete('group');
    history.replaceState(null, '', u.pathname + u.search);
  }}

  sel.addEventListener('change', apply);
  search.addEventListener('input', apply);
  apply();
}})();
</script>
</body>
</html>
"""


def main() -> None:
    rows = load_rows()
    OUT_INDEX.parent.mkdir(parents=True, exist_ok=True)
    OUT_INDEX.write_text(generate_index(rows), encoding="utf-8")
    OUT_FILES.write_text(generate_testfiles(rows), encoding="utf-8")
    print(f"Wrote {OUT_INDEX}")
    print(f"Wrote {OUT_FILES}")


if __name__ == "__main__":
    main()
