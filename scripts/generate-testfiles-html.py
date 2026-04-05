#!/usr/bin/env python3
"""Generate docs/testfiles.html from data/file-results.tsv.

Same data source as index.html — both read from data/.
Run after scripts/run-tests.sh to update results.
"""

import csv
import os
import re
from datetime import datetime, timezone

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DATA_DIR = os.path.join(REPO, "data")
FILE_RESULTS = os.path.join(DATA_DIR, "file-results.tsv")
TEST_RESULTS = os.path.join(DATA_DIR, "test-results.tsv")
TESTS_DIR = os.path.join(REPO, "tests")
OUT = os.path.join(REPO, "docs", "testfiles.html")

CATEGORIES = [
    ("t0", "Basic / Setup"),
    ("t1", "Plumbing: read-tree, cat-file, refs"),
    ("t2", "Checkout / Index"),
    ("t3", "ls-files, merge, cherry-pick, rm, add, mv"),
    ("t4", "Diff"),
    ("t5", "Pack / Fetch / Push / Clone"),
    ("t6", "Rev-list, rev-parse, merge-base, for-each-ref"),
    ("t7", "Porcelain: commit, status, tag, branch, reset"),
    ("t8", "Git-p4 / Misc"),
    ("t9", "Contrib / Completion"),
]


def get_category(filename):
    for prefix, name in CATEGORIES:
        if filename.startswith(prefix):
            return prefix, name
    return "t?", "Other"


def get_description(fname):
    """Extract test_description from a test file."""
    fpath = os.path.join(TESTS_DIR, fname + ".sh")
    if not os.path.exists(fpath):
        fpath = os.path.join(TESTS_DIR, fname)
    try:
        with open(fpath) as f:
            for line in f:
                m = re.search(r"test_description=['\"]([^'\"]*)", line)
                if m:
                    return m.group(1)[:100]
    except Exception:
        pass
    return ""


def load_file_results():
    """Load per-file results from data/file-results.tsv."""
    results = {}
    if not os.path.exists(FILE_RESULTS):
        return results
    with open(FILE_RESULTS) as f:
        reader = csv.DictReader(f, delimiter='\t')
        for row in reader:
            fname = row.get("file", "")
            if not fname:
                continue
            total = int(row.get("total_tests") or row.get("total") or 0)
            passing = int(row.get("passing") or row.get("pass") or 0)
            failing = int(row.get("failing") or row.get("fail") or 0)
            results[fname] = {
                "total": total,
                "pass": passing,
                "fail": failing,
                "skip": 0,
                "status": row.get("status", "unknown"),
                "ported": row.get("ported", "yes") == "yes",
                "real_pass": int(row.get("real_pass", 0)),
                "real_total": int(row.get("real_total", 0)),
                "expect_failure": int(row.get("expect_failure", 0)),
            }
    return results


def load_canonical_counts():
    """Load per-file test counts from data/test-results.tsv (upstream 18,097)."""
    counts = {}
    if not os.path.exists(TEST_RESULTS):
        return counts
    with open(TEST_RESULTS) as f:
        f.readline()  # skip header
        for line in f:
            parts = line.strip().split('\t')
            if len(parts) >= 2:
                counts[parts[1]] = counts.get(parts[1], 0) + 1
    return counts


def pct(n, d):
    return round(100 * n / d, 1) if d > 0 else 0


def bar_color(p):
    if p >= 100: return "#3fb950"
    if p >= 75: return "#58a6ff"
    if p >= 50: return "#d29922"
    if p >= 25: return "#db6d28"
    return "#f85149"


def status_badge(p, total):
    if total == 0: return '<span style="color:#7d8590">—</span>'
    if p >= 100: return '<span style="color:#3fb950">✓ PASS</span>'
    if p >= 75: return f'<span style="color:#58a6ff">{p:.0f}%</span>'
    if p >= 50: return f'<span style="color:#d29922">{p:.0f}%</span>'
    if p > 0: return f'<span style="color:#db6d28">{p:.0f}%</span>'
    return '<span style="color:#f85149">0%</span>'


def generate():
    results = load_file_results()
    canonical = load_canonical_counts()
    
    # Get all files: from results + canonical + test dir
    all_bases = set(results.keys()) | set(canonical.keys())
    for f in os.listdir(TESTS_DIR):
        if re.match(r't\d+.*\.sh$', f):
            all_bases.add(f.replace('.sh', ''))
    
    rows = []
    for base in sorted(all_bases):
        if not re.match(r't\d+', base):
            continue
        
        cat_prefix, cat_name = get_category(base)
        r = results.get(base, {})
        desc = get_description(base)
        is_canonical = base in canonical
        canon_count = canonical.get(base, 0)
        
        total = r.get("total", 0)
        passing = r.get("pass", 0)
        failing = r.get("fail", 0)
        skip = r.get("skip", 0)
        status = r.get("status", "unknown")
        timestamp = r.get("timestamp", "")
        
        p = pct(passing, total) if total > 0 else -1
        
        rows.append({
            "file": base,
            "cat_prefix": cat_prefix,
            "cat_name": cat_name,
            "desc": desc,
            "is_canonical": is_canonical,
            "canon_count": canon_count,
            "total": total,
            "pass": passing,
            "fail": failing,
            "skip": skip,
            "status": status,
            "timestamp": timestamp,
            "pct": p,
        })

    # Summary stats — from file-results.tsv (same source as index.html)
    files_with_results = [r for r in rows if r["total"] > 0]
    total_files = len(files_with_results)
    pass_files = len([r for r in files_with_results if r["fail"] == 0])
    total_pass = sum(r["pass"] for r in files_with_results)
    total_tests = sum(r["total"] for r in files_with_results)
    
    # Upstream-only stats
    canon_rows = [r for r in files_with_results if r["is_canonical"]]
    canon_total_files = len(canon_rows)
    canon_pass_files = len([r for r in canon_rows if r["fail"] == 0])
    canon_pass_tests = sum(r["pass"] for r in canon_rows)
    canon_total_tests = sum(r["total"] for r in canon_rows)

    # Category stats
    cat_stats = {}
    for r in rows:
        cp = r["cat_prefix"]
        if cp not in cat_stats:
            cat_stats[cp] = {"name": r["cat_name"], "files": 0, "pass_files": 0, "tests": 0, "passing": 0}
        if r["total"] > 0:
            cat_stats[cp]["files"] += 1
            cat_stats[cp]["tests"] += r["total"]
            cat_stats[cp]["passing"] += r["pass"]
            if r["fail"] == 0:
                cat_stats[cp]["pass_files"] += 1

    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")

    html = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Grit &mdash; Test Files</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
  background: #0d1117; color: #e6edf3; padding: 2rem;
  max-width: 1400px; margin: 0 auto;
}}
h1 {{ font-size: 1.5rem; margin-bottom: 0.3rem; color: #f0f6fc; }}
.subtitle {{ color: #7d8590; margin-bottom: 1.5rem; font-size: 0.9rem; }}
.subtitle a {{ color: #58a6ff; text-decoration: none; }}
.subtitle a:hover {{ text-decoration: underline; }}

.summary-cards {{
  display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: 1rem; margin-bottom: 1.5rem;
}}
.card {{
  background: #161b22; border: 1px solid #30363d; border-radius: 8px;
  padding: 1rem; text-align: center;
}}
.card .num {{ font-size: 1.8rem; font-weight: 700; color: #f0f6fc; }}
.card .label {{ font-size: 0.8rem; color: #7d8590; margin-top: 0.2rem; }}

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
  position: sticky; top: 0; background: #0d1117; z-index: 1; cursor: pointer;
}}
th:hover {{ color: #e6edf3; }}
td {{ padding: 0.45rem 0.6rem; font-size: 0.85rem; border-bottom: 1px solid #161b22; }}
tr:hover td {{ background: #161b22; }}
.mono {{ font-family: 'SF Mono', Consolas, monospace; font-size: 0.82rem; }}
.bar-cell {{ width: 120px; }}
.bar-bg {{ background: #21262d; border-radius: 3px; height: 8px; overflow: hidden; }}
.bar-fg {{ height: 100%; border-radius: 3px; transition: width 0.3s; }}
.right {{ text-align: right; }}
.desc {{ color: #7d8590; max-width: 350px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
.canonical {{ }}
.grit-only {{ opacity: 0.5; }}
</style>
</head>
<body>
<h1>Test Files</h1>
<p class="subtitle">
  <a href="index.html">&larr; Dashboard</a> &middot;
  <a href="tests.html">All Test Cases</a> &middot;
  <a href="timeline.html">Timeline</a>
  &nbsp;&mdash;&nbsp; Generated {now}
</p>

<div class="summary-cards">
  <div class="card"><div class="num">{total_files:,}</div><div class="label">Test Files</div></div>
  <div class="card"><div class="num" style="color:#3fb950">{pass_files:,}</div><div class="label">Fully Passing</div></div>
  <div class="card"><div class="num">{canon_pass_tests:,}</div><div class="label">Tests Passing</div></div>
  <div class="card"><div class="num">{sum(canonical.values()):,}</div><div class="label">Upstream Tests</div></div>
  <div class="card"><div class="num" style="color:{'#3fb950' if total_tests else '#7d8590'}">{pct(canon_pass_tests, sum(canonical.values())):.1f}%</div><div class="label">Pass Rate</div></div>
</div>
"""

    # Category buttons
    html += '<div style="display:flex;flex-wrap:wrap;gap:0.5rem;margin-bottom:1.5rem;">\n'
    for cp in sorted(cat_stats.keys()):
        cs = cat_stats[cp]
        p = pct(cs["passing"], cs["tests"])
        html += f'  <button class="filter-btn" onclick="filterCat(\'{cp}\')" data-cat="{cp}" title="{cs["name"]}: {cs["passing"]}/{cs["tests"]} ({p:.0f}%)">{cp}xxx &mdash; {p:.0f}%</button>\n'
    html += '  <button class="filter-btn" onclick="filterCat(\'\')" data-cat="">All</button>\n'
    html += '</div>\n'

    html += """
<div class="controls">
  <input type="text" id="search" placeholder="Search file name or description…" oninput="applyFilters()">
  <select id="statusFilter" onchange="applyFilters()">
    <option value="">All statuses</option>
    <option value="pass">✓ Fully passing</option>
    <option value="partial">Partial</option>
    <option value="fail">Failing (0%)</option>
    <option value="norun">No results</option>
  </select>
  <select id="sourceFilter" onchange="applyFilters()">
    <option value="">All sources</option>
    <option value="upstream">Upstream only</option>
    <option value="grit">Grit-specific only</option>
  </select>
  <span class="count" id="rowCount"></span>
</div>

<table id="mainTable">
<thead>
<tr>
  <th onclick="sortTable(0)">File</th>
  <th onclick="sortTable(1)">Description</th>
  <th onclick="sortTable(2)" class="right">Tests</th>
  <th onclick="sortTable(3)" class="right">Pass</th>
  <th onclick="sortTable(4)" class="right">Fail</th>
  <th onclick="sortTable(5)" class="right">Skip</th>
  <th onclick="sortTable(6)">Progress</th>
  <th onclick="sortTable(7)" class="right">Rate</th>
</tr>
</thead>
<tbody>
"""

    for r in rows:
        p = r["pct"]
        color = bar_color(p) if p >= 0 else "#21262d"
        width = max(p, 0)
        badge = status_badge(p, r["total"]) if r["total"] > 0 else '<span style="color:#7d8590">—</span>'
        row_class = "canonical" if r["is_canonical"] else "grit-only"
        source = "upstream" if r["is_canonical"] else "grit"

        html += f'''<tr class="{row_class}" data-cat="{r['cat_prefix']}" data-pct="{p}" data-total="{r['total']}" data-source="{source}">
  <td class="mono">{r['file']}</td>
  <td class="desc" title="{r['desc']}">{r['desc']}</td>
  <td class="right mono">{r['total'] if r['total'] > 0 else ''}</td>
  <td class="right mono" style="color:#3fb950">{r['pass'] if r['total'] > 0 else ''}</td>
  <td class="right mono" style="color:#f85149">{r['fail'] if r['fail'] > 0 else ''}</td>
  <td class="right mono" style="color:#7d8590">{r['skip'] if r['skip'] > 0 else ''}</td>
  <td class="bar-cell"><div class="bar-bg"><div class="bar-fg" style="width:{width}%;background:{color}"></div></div></td>
  <td class="right">{badge}</td>
</tr>
'''

    html += """</tbody>
</table>

<script>
let sortCol = -1, sortAsc = true;

function sortTable(col) {
  const tbody = document.querySelector('#mainTable tbody');
  const rows = Array.from(tbody.querySelectorAll('tr'));
  if (sortCol === col) sortAsc = !sortAsc;
  else { sortCol = col; sortAsc = col <= 1 ? true : false; }
  
  rows.sort((a, b) => {
    let va = a.cells[col].textContent.trim();
    let vb = b.cells[col].textContent.trim();
    if (col >= 2) {
      va = parseFloat(va) || 0;
      vb = parseFloat(vb) || 0;
      return sortAsc ? va - vb : vb - va;
    }
    return sortAsc ? va.localeCompare(vb) : vb.localeCompare(va);
  });
  rows.forEach(r => tbody.appendChild(r));
}

let activeCat = '';
function filterCat(cat) {
  activeCat = cat;
  document.querySelectorAll('.filter-btn').forEach(b => {
    b.classList.toggle('active', b.dataset.cat === cat);
  });
  applyFilters();
}

function applyFilters() {
  const search = document.getElementById('search').value.toLowerCase();
  const status = document.getElementById('statusFilter').value;
  const source = document.getElementById('sourceFilter').value;
  const rows = document.querySelectorAll('#mainTable tbody tr');
  let visible = 0;
  
  rows.forEach(row => {
    const cat = row.dataset.cat;
    const pct = parseFloat(row.dataset.pct);
    const total = parseInt(row.dataset.total);
    const rowSource = row.dataset.source;
    const text = (row.cells[0].textContent + ' ' + row.cells[1].textContent).toLowerCase();
    
    let show = true;
    if (activeCat && cat !== activeCat) show = false;
    if (search && !text.includes(search)) show = false;
    if (status === 'pass' && (pct < 100 || total === 0)) show = false;
    if (status === 'partial' && (pct <= 0 || pct >= 100 || total === 0)) show = false;
    if (status === 'fail' && (pct !== 0 || total === 0)) show = false;
    if (status === 'norun' && total > 0) show = false;
    if (source && rowSource !== source) show = false;
    
    row.style.display = show ? '' : 'none';
    if (show) visible++;
  });
  
  document.getElementById('rowCount').textContent = visible + ' / ' + rows.length + ' files';
}

applyFilters();
</script>
</body>
</html>
"""

    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, 'w') as f:
        f.write(html)
    
    print(f"Wrote {OUT}")
    print(f"  {total_pass:,}/{total_tests:,} tests passing ({pct(total_pass, total_tests):.1f}%)")
    print(f"  {pass_files} / {total_files} files fully passing")


if __name__ == "__main__":
    generate()
