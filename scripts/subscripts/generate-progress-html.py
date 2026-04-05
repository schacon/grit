#!/usr/bin/env python3
"""
Script 3: Generate docs/index.html from command-status.tsv and test-results.tsv.
"""

import os
import subprocess
from datetime import datetime, timezone

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DATA = os.path.join(REPO, "data")
OUT = os.path.join(REPO, "docs", "index.html")


def load_command_status():
    path = os.path.join(DATA, "command-status.tsv")
    commands = []
    with open(path) as f:
        f.readline()
        for line in f:
            parts = line.strip().split('\t')
            if len(parts) < 7:
                continue
            commands.append({
                'name': parts[0],
                'category': parts[1],
                'started': parts[2] == 'yes',
                'total_tests': int(parts[3]),
                'passing': int(parts[4]),
                'not_ported': int(parts[5]),
                'pct': float(parts[6]),
            })
    return commands


def load_test_results_summary():
    """Get overall upstream test stats."""
    path = os.path.join(DATA, "test-results.tsv")
    total = 0
    passing = 0
    partial = 0
    not_ported = 0
    with open(path) as f:
        f.readline()
        for line in f:
            parts = line.strip().split('\t')
            if len(parts) < 5:
                continue
            total += 1
            s = parts[4]
            if s == 'pass':
                passing += 1
            elif s == 'partial':
                partial += 1
            else:
                not_ported += 1
    return total, passing, partial, not_ported


def load_ported_test_stats():
    """Get ported test suite totals from file-results.tsv."""
    path = os.path.join(DATA, "file-results.tsv")
    total_tests = 0
    total_pass = 0
    ported_files = 0
    fully_passing_files = 0
    with open(path) as f:
        f.readline()
        for line in f:
            parts = line.strip().split('\t')
            if len(parts) < 6:
                continue
            if parts[1] == 'yes':
                ported_files += 1
                t = int(parts[2])
                p = int(parts[3])
                fl = int(parts[4])
                total_tests += t
                total_pass += p
                if t > 0 and fl == 0:
                    fully_passing_files += 1
    return ported_files, total_tests, total_pass, fully_passing_files


def generate_html(commands, total_upstream, upstream_passing, upstream_partial,
                  upstream_not_ported, ported_files, ported_tests, ported_pass,
                  fully_passing_files):
    total_cmds = len(commands)
    started_cmds = sum(1 for c in commands if c['started'])
    partially_impl = sum(1 for c in commands if c['started'] and 0 < c['pct'] < 100)
    complete_cmds = sum(1 for c in commands if c['started'] and c['total_tests'] > 0 and c['pct'] == 100)
    pct_cmds = round(100 * started_cmds / total_cmds, 1) if total_cmds > 0 else 0

    # Derive headline totals by summing command-status so they match per-command bars
    sum_passing = ported_pass  # real_pass from load_ported_test_stats()
    sum_total = 18097  # total upstream test cases
    raw_pct = round(100 * sum_passing / sum_total, 1) if sum_total > 0 else 0

    # Timestamp and commit info
    now = datetime.now(timezone.utc)
    gen_timestamp = int(now.timestamp())
    gen_time_str = now.strftime('%Y-%m-%d %H:%M UTC')
    try:
        commit_sha = subprocess.check_output(
            ['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True
        ).strip()
    except Exception:
        commit_sha = 'unknown'
    commit_sha_short = commit_sha[:7]

    # Group commands by category
    categories = {}
    for cmd in commands:
        cat = cmd['category']
        if cat not in categories:
            categories[cat] = []
        categories[cat].append(cmd)

    cat_order = ["Main Porcelain", "Ancillary Porcelain", "Plumbing"]

    # Build command grid HTML
    grid_html = ""
    for cat in cat_order:
        cmds = categories.get(cat, [])
        started_in_cat = sum(1 for c in cmds if c['started'])
        complete_in_cat = sum(1 for c in cmds if c['started'] and c['total_tests'] > 0 and c['pct'] == 100)
        cat_passing = sum(c['passing'] for c in cmds)
        cat_total = sum(c['total_tests'] for c in cmds)
        cat_pct = round(100 * cat_passing / cat_total, 1) if cat_total > 0 else 0
        grid_html += f'<div class="cat-section"><h3>{cat} <span class="cat-count">{complete_in_cat}/{len(cmds)} complete</span></h3>\n'
        grid_html += f'  <div style="margin-bottom:0.6rem;"><div style="display:flex;align-items:center;gap:0.6rem;">'
        grid_html += f'<div style="flex:1;background:#21262d;border-radius:4px;height:14px;overflow:hidden;border:1px solid #30363d;">'
        grid_html += f'<div style="width:{cat_pct}%;height:100%;background:linear-gradient(90deg,#238636,#2ea043);border-radius:4px 0 0 4px;"></div></div>'
        grid_html += f'<span style="font-size:0.78rem;color:#7d8590;white-space:nowrap;">{cat_passing:,} / {cat_total:,} ({cat_pct}%)</span>'
        grid_html += f'</div></div>\n'
        grid_html += '<div class="cmd-grid">\n'
        for cmd in cmds:
            if not cmd['started']:
                cls = "cmd-cell not-started"
                tooltip = f"{cmd['name']}: not started"
                badge = ""
            elif cmd['total_tests'] == 0:
                cls = "cmd-cell started-no-tests"
                tooltip = f"{cmd['name']}: started (no associated upstream tests)"
                badge = ""
            elif cmd['pct'] == 100:
                cls = "cmd-cell complete"
                tooltip = f"{cmd['name']}: all {cmd['total_tests']} associated tests passing"
                badge = f'<span class="cmd-pct">{cmd["passing"]}/{cmd["total_tests"]}</span>'
            elif cmd['passing'] == 0:
                cls = "cmd-cell zero-pass"
                tooltip = f"{cmd['name']}: 0/{cmd['total_tests']} associated tests passing"
                badge = f'<span class="cmd-pct">0/{cmd["total_tests"]}</span>'
            else:
                cls = "cmd-cell partial"
                tooltip = f"{cmd['name']}: {cmd['passing']}/{cmd['total_tests']} associated tests passing ({cmd['pct']}%)"
                badge = f'<span class="cmd-pct">{cmd["passing"]}/{cmd["total_tests"]}</span>'
            bar = ""
            if cls == "cmd-cell partial":
                p = cmd['pct']
                if p <= 5:
                    bar_cls = "bar-red"
                elif p <= 50:
                    bar_cls = "bar-orange"
                elif p <= 85:
                    bar_cls = "bar-blue"
                else:
                    bar_cls = "bar-green"
                bar = f'<div class="cmd-bar {bar_cls}" style="width:{p}%"></div>'
            if cmd['total_tests'] > 0:
                grid_html += f'  <a href="tests.html?cmd={cmd["name"]}" class="{cls}" title="{tooltip}">'
                grid_html += f'{bar}<span class="cmd-name">{cmd["name"]}</span>{badge}</a>\n'
            else:
                grid_html += f'  <div class="{cls}" title="{tooltip}">'
                grid_html += f'{bar}<span class="cmd-name">{cmd["name"]}</span>{badge}</div>\n'
        grid_html += '</div></div>\n'

    html = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Grit Progress Dashboard</title>
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
h1 {{
  font-size: 1.8rem;
  margin-bottom: 0.3rem;
  color: #f0f6fc;
}}
.subtitle {{
  color: #7d8590;
  margin-bottom: 2rem;
  font-size: 0.95rem;
}}

/* ── Summary cards ── */
.cards {{
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
  gap: 1rem;
  margin-bottom: 2rem;
}}
.card {{
  background: #161b22;
  border: 1px solid #30363d;
  border-radius: 8px;
  padding: 1.2rem;
  text-align: center;
}}
.card .number {{
  font-size: 2rem;
  font-weight: 700;
  line-height: 1.2;
  white-space: nowrap;
}}
.card .label {{
  font-size: 0.78rem;
  color: #7d8590;
  margin-top: 0.3rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}}
.card.green .number {{ color: #3fb950; }}
.card.blue .number {{ color: #58a6ff; }}
.card.yellow .number {{ color: #d29922; }}
.card.purple .number {{ color: #bc8cff; }}

/* ── Progress bar ── */
.progress-section {{
  margin-bottom: 2.5rem;
}}
.progress-section h2 {{
  font-size: 1.1rem;
  margin-bottom: 0.8rem;
  color: #f0f6fc;
}}
.progress-bar-bg {{
  background: #21262d;
  border-radius: 8px;
  height: 40px;
  overflow: hidden;
  position: relative;
  border: 1px solid #30363d;
}}
.progress-bar-fill {{
  height: 100%;
  display: flex;
  align-items: center;
  font-size: 0.85rem;
  font-weight: 600;
  color: #fff;
  transition: width 0.3s ease;
}}
.progress-bar-fill.green {{
  background: linear-gradient(90deg, #238636, #2ea043);
  border-radius: 8px 0 0 8px;
}}
.progress-bar-fill.yellow {{
  background: linear-gradient(90deg, #9e6a03, #bb8009);
}}
.progress-labels {{
  display: flex;
  justify-content: space-between;
  margin-top: 0.5rem;
  font-size: 0.82rem;
  color: #7d8590;
}}

/* ── Stat row ── */
.stat-row {{
  display: flex;
  gap: 2rem;
  margin-bottom: 2rem;
  flex-wrap: wrap;
  font-size: 0.9rem;
  color: #7d8590;
}}
.stat-row .stat {{
  display: flex;
  gap: 0.5rem;
  align-items: baseline;
}}
.stat-row .stat strong {{
  color: #e6edf3;
  font-size: 1rem;
}}

/* ── Command grid ── */
.grid-section {{
  margin-bottom: 2rem;
}}
.grid-section > h2 {{
  font-size: 1.2rem;
  margin-bottom: 0.5rem;
  color: #f0f6fc;
}}
.legend {{
  display: flex;
  gap: 1.5rem;
  margin-bottom: 1.5rem;
  flex-wrap: wrap;
  font-size: 0.82rem;
  color: #7d8590;
}}
.legend-item {{
  display: flex;
  align-items: center;
  gap: 0.4rem;
}}
.legend-swatch {{
  width: 14px;
  height: 14px;
  border-radius: 3px;
  border: 1px solid #30363d;
}}

.cat-section {{
  margin-bottom: 1.8rem;
}}
.cat-section h3 {{
  font-size: 0.9rem;
  color: #7d8590;
  margin-bottom: 0.6rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  font-weight: 500;
}}
.cat-count {{
  font-weight: 400;
  font-size: 0.8rem;
  color: #484f58;
  text-transform: none;
  letter-spacing: 0;
}}
.cmd-grid {{
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(185px, 1fr));
  gap: 6px;
}}
.cmd-cell {{
  border-radius: 6px;
  padding: 0.45rem 0.6rem;
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 0.82rem;
  border: 1px solid transparent;
  cursor: default;
  transition: transform 0.1s;
  white-space: nowrap;
  position: relative;
  overflow: hidden;
}}
.cmd-cell:hover {{
  transform: scale(1.04);
  z-index: 1;
}}
a.cmd-cell {{
  text-decoration: none;
  color: inherit;
}}
.cmd-name {{
  font-weight: 600;
  font-family: 'SF Mono', 'Cascadia Code', 'Fira Code', monospace;
  font-size: 0.76rem;
  position: relative;
  z-index: 1;
}}
.cmd-pct {{
  font-size: 0.68rem;
  opacity: 0.8;
  font-family: 'SF Mono', 'Cascadia Code', 'Fira Code', monospace;
  position: relative;
  z-index: 1;
}}
.cmd-bar {{
  position: absolute;
  top: 0;
  left: 0;
  bottom: 0;
  border-radius: 6px 0 0 6px;
  z-index: 0;
}}
.cmd-bar.bar-red    {{ background: linear-gradient(90deg, #3d1214, #5a1a1e); }}
.cmd-bar.bar-orange {{ background: linear-gradient(90deg, #3a2310, #4a2e14); }}
.cmd-bar.bar-blue   {{ background: linear-gradient(90deg, #152238, #1a3050); }}
.cmd-bar.bar-green  {{ background: linear-gradient(90deg, #1a3a1a, #12361e); }}

/* Cell colors */
.cmd-cell.not-started {{
  background: #161b22;
  border-color: #21262d;
  color: #484f58;
}}
.cmd-cell.started-no-tests {{
  background: #1c2333;
  border-color: #1f3a5f;
  color: #58a6ff;
}}
.cmd-cell.complete {{
  background: #12261e;
  border-color: #238636;
  color: #3fb950;
}}
.cmd-cell.partial {{
  background: #161b22;
  border-color: #30363d;
  color: #e6edf3;
}}
.cmd-cell.zero-pass {{
  background: #161b22;
  border-color: #21262d;
  color: #484f58;
}}

/* ── Footer ── */
.footer {{
  margin-top: 3rem;
  padding-top: 1.5rem;
  border-top: 1px solid #21262d;
  font-size: 0.8rem;
  color: #484f58;
  text-align: center;
}}
.footer a {{ color: #58a6ff; text-decoration: none; }}
.footer a:hover {{ text-decoration: underline; }}
</style>
</head>
<body>

<h1>Grit &mdash; Git Reimplementation Progress</h1>
<p class="subtitle">Test-driven compatibility tracking against the upstream Git test suite
  &middot; <span id="gen-time" data-ts="{gen_timestamp}">{gen_time_str}</span>
  &middot; <a href="https://github.com/schacon/grit/commit/{commit_sha}" style="color:#484f58">{commit_sha_short}</a>
  &middot; <a href="testfiles.html" style="color:#58a6ff">Test Files</a>
  &middot; <a href="timeline.html" style="color:#58a6ff">Timeline</a>
</p>

<div class="cards">
  <div class="card green">
    <div class="number">{sum_passing:,}</div>
    <div class="label">Tests Passing</div>
  </div>
  <div class="card blue">
    <div class="number">{sum_total:,}</div>
    <div class="label">Total Git Test Cases</div>
  </div>
  <div class="card green">
    <div class="number">{raw_pct}%</div>
    <div class="label">Passing Rate</div>
  </div>
  <div class="card yellow">
    <div class="number">{partially_impl}</div>
    <div class="label">Partially Implemented</div>
  </div>
  <div class="card purple">
    <div class="number">{started_cmds} / {total_cmds}</div>
    <div class="label">Commands Started</div>
  </div>
  <div class="card purple">
    <div class="number">{pct_cmds}%</div>
    <div class="label">of Subcommands</div>
  </div>
</div>

<div class="progress-section">
  <h2>Overall Test Progress</h2>
  <div class="progress-bar-bg">
    <div style="display:flex;width:100%;height:100%;">
      <div class="progress-bar-fill green" style="width:{raw_pct}%;justify-content:center;">
        {sum_passing:,} passing
      </div>
    </div>
  </div>
  <div class="progress-labels">
    <span style="color:#3fb950;">{sum_passing:,} / {sum_total:,} upstream tests passing ({raw_pct}%)</span>
    <span>{ported_files} test files ported · {fully_passing_files} fully passing</span>
  </div>
</div>

<div class="grid-section">
  <h2>Command Implementation Status</h2>
  <div class="legend">
    <div class="legend-item"><div class="legend-swatch" style="background:#12261e;border-color:#238636;"></div> Complete (all associated tests pass)</div>
    <div class="legend-item"><div class="legend-swatch" style="background:#5a1a1e;border-color:#30363d;"></div> &lt;5%</div>
    <div class="legend-item"><div class="legend-swatch" style="background:#4a2e14;border-color:#30363d;"></div> 6&ndash;50%</div>
    <div class="legend-item"><div class="legend-swatch" style="background:#1a3050;border-color:#30363d;"></div> 51&ndash;85%</div>
    <div class="legend-item"><div class="legend-swatch" style="background:#12361e;border-color:#30363d;"></div> &gt;85%</div>
    <div class="legend-item"><div class="legend-swatch" style="background:#1c2333;border-color:#1f3a5f;"></div> Started (no associated tests found)</div>
    <div class="legend-item"><div class="legend-swatch" style="background:#161b22;border-color:#21262d;"></div> Not started</div>
  </div>
{grid_html}
</div>

<div class="footer">
  <a href="tests.html">View all test cases</a> &middot;
  <a href="testfiles.html">Test files</a> &middot;
  Generated by <a href="https://github.com/schacon/grit">grit</a> progress scripts
</div>
<script>
(function() {{
  var el = document.getElementById('gen-time');
  if (!el) return;
  var ts = parseInt(el.dataset.ts, 10);
  var now = Date.now() / 1000;
  var diff = Math.floor(now - ts);
  var text;
  if (diff < 60) text = 'just now';
  else if (diff < 3600) text = Math.floor(diff/60) + 'm ago';
  else if (diff < 86400) text = Math.floor(diff/3600) + 'h ago';
  else text = Math.floor(diff/86400) + 'd ago';
  el.textContent = text;
}})();
</script>

</body>
</html>"""

    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, 'w') as f:
        f.write(html)
    print(f"Wrote {OUT}")


def load_all_test_cases():
    """Load every test case from test-results.tsv."""
    path = os.path.join(DATA, "test-results.tsv")
    tests = []
    with open(path) as f:
        f.readline()
        for line in f:
            parts = line.rstrip('\n').split('\t')
            if len(parts) < 5:
                continue
            tests.append({
                'id': parts[0],
                'file': parts[1],
                'desc': parts[2],
                'subcmds': parts[3],
                'status': parts[4],
            })
    return tests


def generate_tests_html(tests):
    """Generate docs/tests.html with a filterable table of every test case."""
    out_path = os.path.join(REPO, "docs", "tests.html")

    total = len(tests)
    passing = sum(1 for t in tests if t['status'] == 'pass')
    partial = sum(1 for t in tests if t['status'] == 'partial')
    not_ported = sum(1 for t in tests if t['status'] == 'not_ported')

    # Group by file for collapsible sections
    files = {}
    for t in tests:
        files.setdefault(t['file'], []).append(t)

    def esc(s):
        return s.replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;').replace('"', '&quot;')

    all_cmds = set()
    rows_html = ""
    for fbase in sorted(files.keys()):
        ftests = files[fbase]
        for t in ftests:
            if t['status'] == 'pass':
                icon = '<span class="status-icon pass">&#10003;</span>'
            elif t['status'] == 'partial':
                icon = '<span class="status-icon partial">&#9679;</span>'
            else:
                icon = '<span class="status-icon not-ported">&mdash;</span>'
            subcmds_html = ""
            cmds_list = [c.strip() for c in t['subcmds'].split(',') if c.strip()] if t['subcmds'] else []
            all_cmds.update(cmds_list)
            if cmds_list:
                subcmds_html = " ".join(
                    f'<span class="tag">{esc(c)}</span>' for c in cmds_list
                )
            cmds_data = ",".join(cmds_list)
            rows_html += (
                f'<tr class="row-{t["status"]}" data-status="{t["status"]}" '
                f'data-file="{esc(t["file"])}" data-cmds="{esc(cmds_data)}">'
                f'<td>{icon}</td>'
                f'<td class="cell-file">{esc(t["file"])}</td>'
                f'<td class="cell-desc">{esc(t["desc"])}</td>'
                f'<td class="cell-cmds">{subcmds_html}</td>'
                f'</tr>\n'
            )

    cmd_options = "\n".join(
        f'    <option value="{esc(c)}">{esc(c)}</option>' for c in sorted(all_cmds)
    )

    html = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Grit &mdash; All Test Cases</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
  background: #0d1117;
  color: #e6edf3;
  padding: 2rem;
  max-width: 1400px;
  margin: 0 auto;
}}
h1 {{ font-size: 1.5rem; margin-bottom: 0.3rem; color: #f0f6fc; }}
.subtitle {{ color: #7d8590; margin-bottom: 1.5rem; font-size: 0.9rem; }}
.subtitle a {{ color: #58a6ff; text-decoration: none; }}
.subtitle a:hover {{ text-decoration: underline; }}

.controls {{
  display: flex; gap: 1rem; margin-bottom: 1rem; flex-wrap: wrap; align-items: center;
}}
.controls input, .controls select {{
  background: #161b22; border: 1px solid #30363d; border-radius: 6px;
  color: #e6edf3; padding: 0.5rem 0.8rem; font-size: 0.85rem;
}}
.controls input {{ width: 300px; }}
.controls select {{ min-width: 160px; cursor: pointer; }}
.controls input:focus, .controls select:focus {{ outline: none; border-color: #58a6ff; }}
.cmd-filter-label {{
  color: #7d8590; font-size: 0.82rem; display: flex; align-items: center; gap: 0.4rem;
}}
.filter-btn {{
  background: #21262d; border: 1px solid #30363d; border-radius: 6px;
  color: #7d8590; padding: 0.4rem 0.8rem; font-size: 0.82rem; cursor: pointer;
}}
.filter-btn:hover {{ border-color: #58a6ff; color: #e6edf3; }}
.filter-btn.active {{ background: #1f3a5f; border-color: #58a6ff; color: #58a6ff; }}
.count {{ color: #7d8590; font-size: 0.82rem; margin-left: auto; }}

table {{ width: 100%; border-collapse: collapse; }}
th {{
  text-align: left; padding: 0.5rem 0.6rem; font-size: 0.75rem;
  color: #7d8590; border-bottom: 1px solid #21262d;
  text-transform: uppercase; letter-spacing: 0.05em;
  position: sticky; top: 0; background: #0d1117; z-index: 1;
}}
td {{
  padding: 0.35rem 0.6rem; font-size: 0.82rem; border-bottom: 1px solid #161b22;
  vertical-align: top;
}}
tr:hover td {{ background: #161b22; }}
.cell-file {{
  font-family: 'SF Mono', 'Cascadia Code', 'Fira Code', monospace;
  font-size: 0.75rem; color: #7d8590; white-space: nowrap;
}}
.cell-desc {{ max-width: 500px; }}
.cell-cmds {{ white-space: normal; }}

.status-icon {{ font-size: 0.85rem; }}
.status-icon.pass {{ color: #3fb950; }}
.status-icon.partial {{ color: #d29922; }}
.status-icon.not-ported {{ color: #484f58; }}

.tag {{
  display: inline-block; background: #21262d; border-radius: 3px;
  padding: 0.1rem 0.35rem; font-size: 0.7rem; margin: 1px 2px;
  font-family: 'SF Mono', 'Cascadia Code', 'Fira Code', monospace;
  color: #7d8590;
}}
tr.row-pass .tag {{ background: #12261e; color: #3fb950; }}
tr.row-partial .tag {{ background: #272115; color: #d29922; }}

.hidden {{ display: none; }}

.footer {{
  margin-top: 2rem; padding-top: 1rem; border-top: 1px solid #21262d;
  font-size: 0.8rem; color: #484f58; text-align: center;
}}
.footer a {{ color: #58a6ff; text-decoration: none; }}
</style>
</head>
<body>

<h1>All Upstream Test Cases</h1>
<p class="subtitle"><a href="index.html">&larr; Back to dashboard</a> &middot; {total:,} tests &middot; {passing:,} pass &middot; {partial:,} partial &middot; {not_ported:,} not ported</p>

<div class="controls">
  <input type="text" id="search" placeholder="Filter by description, file, or command...">
  <button class="filter-btn active" data-filter="all">All</button>
  <button class="filter-btn" data-filter="pass">Passing</button>
  <button class="filter-btn" data-filter="partial">Partial</button>
  <button class="filter-btn" data-filter="not_ported">Not ported</button>
  <label class="cmd-filter-label">Command:
    <select id="cmd-filter">
      <option value="">All commands</option>
{cmd_options}
    </select>
  </label>
  <span class="count" id="visible-count">{total:,} shown</span>
</div>

<table>
<thead>
<tr><th style="width:30px;"></th><th style="width:160px;">File</th><th>Description</th><th style="width:250px;">Subcommands</th></tr>
</thead>
<tbody id="test-body">
{rows_html}</tbody>
</table>

<div class="footer">
  <a href="index.html">Dashboard</a> &middot;
  Generated by <a href="https://github.com/schacon/grit">grit</a> progress scripts
</div>

<script>
(function() {{
  const rows = document.querySelectorAll('#test-body tr');
  const search = document.getElementById('search');
  const buttons = document.querySelectorAll('.filter-btn');
  const cmdSelect = document.getElementById('cmd-filter');
  const countEl = document.getElementById('visible-count');
  let activeFilter = 'all';

  function applyFilters() {{
    const q = search.value.toLowerCase();
    const cmd = cmdSelect.value;
    let shown = 0;
    rows.forEach(row => {{
      const status = row.dataset.status;
      const text = row.textContent.toLowerCase();
      const cmds = row.dataset.cmds || '';
      const matchFilter = activeFilter === 'all' || status === activeFilter;
      const matchSearch = !q || text.includes(q);
      const matchCmd = !cmd || cmds.split(',').includes(cmd);
      if (matchFilter && matchSearch && matchCmd) {{
        row.classList.remove('hidden');
        shown++;
      }} else {{
        row.classList.add('hidden');
      }}
    }});
    countEl.textContent = shown.toLocaleString() + ' shown';
  }}

  search.addEventListener('input', applyFilters);
  cmdSelect.addEventListener('change', () => {{
    // update URL without reload
    const url = new URL(window.location);
    if (cmdSelect.value) {{
      url.searchParams.set('cmd', cmdSelect.value);
    }} else {{
      url.searchParams.delete('cmd');
    }}
    history.replaceState(null, '', url);
    applyFilters();
  }});
  buttons.forEach(btn => {{
    btn.addEventListener('click', () => {{
      buttons.forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      activeFilter = btn.dataset.filter;
      applyFilters();
    }});
  }});

  // Read ?cmd= from URL on load
  const params = new URLSearchParams(window.location.search);
  const initCmd = params.get('cmd');
  if (initCmd && cmdSelect.querySelector('option[value="' + CSS.escape(initCmd) + '"]')) {{
    cmdSelect.value = initCmd;
  }}
  applyFilters();
}})();
</script>

</body>
</html>"""

    with open(out_path, 'w') as f:
        f.write(html)
    print(f"Wrote {out_path}")


def main():
    commands = load_command_status()
    total_upstream, upstream_passing, upstream_partial, upstream_not_ported = load_test_results_summary()
    ported_files, ported_tests, ported_pass, fully_passing_files = load_ported_test_stats()
    generate_html(commands, total_upstream, upstream_passing, upstream_partial,
                  upstream_not_ported, ported_files, ported_tests, ported_pass,
                  fully_passing_files)

    tests = load_all_test_cases()
    generate_tests_html(tests)


if __name__ == '__main__':
    main()
