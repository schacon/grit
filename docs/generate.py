#!/usr/bin/env python3
"""Generate docs/index.html from the current test state.

Run from the repo root:
    python3 docs/generate.py
"""

import os
import re
import html

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
GIT_TEST_DIR = os.path.join(REPO_ROOT, "git", "t")
OUR_TEST_DIR = os.path.join(REPO_ROOT, "tests")
SELECTED_FILE = os.path.join(OUR_TEST_DIR, "harness", "selected-tests.txt")
OUTPUT = os.path.join(REPO_ROOT, "docs", "index.html")
GITHUB_BASE = "https://github.com/schacon/grit"
SRC_BASE = f"{GITHUB_BASE}/tree/main"


def get_all_upstream_tests():
    """Return sorted list of all upstream test script names."""
    tests = []
    for f in os.listdir(GIT_TEST_DIR):
        if f.startswith("t") and f.endswith(".sh") and f[1:5].isdigit():
            tests.append(f)
    return sorted(tests)


def get_passing_tests():
    """Return set of test names listed in selected-tests.txt."""
    passing = set()
    with open(SELECTED_FILE) as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#"):
                passing.add(line)
    return passing


def get_ported_tests():
    """Return set of test names that exist in our tests/ directory."""
    ported = set()
    for f in os.listdir(OUR_TEST_DIR):
        if f.startswith("t") and f.endswith(".sh"):
            ported.add(f)
    return ported


def get_test_description(test_dir, name):
    """Extract test_description from a test script."""
    path = os.path.join(test_dir, name)
    try:
        with open(path) as f:
            content = f.read(2000)
        m = re.search(r"test_description=['\"](.+?)['\"]", content)
        if m:
            return m.group(1)
    except Exception:
        pass
    # Fall back to name
    return name[6:-3].replace("-", " ")


def guess_command(name):
    """Map a test filename to a primary Git command."""
    n = int(name[1:5])
    
    # Known mappings from test number ranges
    mappings = [
        (0, 1, "init"), (2, 7, "setup"), (8, 8, "check-ignore"),
        (9, 99, "setup"),
        (1000, 1019, "read-tree"), (1020, 1029, "subdirectory"),
        (1100, 1199, "commit-tree"),
        (1300, 1359, "config"), (1400, 1410, "update-ref"),
        (1401, 1401, "symbolic-ref"), (1403, 1403, "show-ref"),
        (1404, 1404, "update-ref"), (1422, 1422, "show-ref"),
        (1500, 1519, "rev-parse"),
        (2000, 2009, "checkout"), (2010, 2029, "checkout"),
        (2060, 2069, "switch"), (2070, 2079, "restore"),
        (2080, 2089, "checkout"),
        (2100, 2199, "update-index"),
        (2200, 2209, "add"),
        (3000, 3019, "ls-files"), (3100, 3109, "ls-tree"),
        (3200, 3209, "branch"),
        (3400, 3499, "rebase"), (3500, 3519, "cherry-pick"),
        (3600, 3609, "rm"), (3700, 3709, "add"),
        (3900, 3909, "stash/i18n"),
        (4000, 4099, "diff"), (4100, 4199, "apply"),
        (4200, 4219, "log"),
        (5000, 5009, "write-tree"),
        (5300, 5399, "pack/prune"),
        (5400, 5419, "send-pack"),
        (5500, 5599, "fetch/push/pull"),
        (5600, 5629, "clone"),
        (5700, 5799, "protocol"),
        (6000, 6029, "rev-list"), (6010, 6010, "merge-base"),
        (6100, 6109, "rev-parse"),
        (6200, 6209, "fmt-merge-msg"),
        (6300, 6309, "for-each-ref"),
        (6400, 6499, "merge"), (6500, 6509, "gc"),
        (7000, 7009, "mv"), (7100, 7119, "reset"),
        (7400, 7499, "submodule"),
        (7500, 7529, "commit/status"),
        (7600, 7619, "merge"), (7700, 7709, "repack"),
        (7800, 7899, "grep"),
        (9000, 9999, "foreign-scm"),
    ]
    
    # Try specific first
    for lo, hi, cmd in sorted(mappings, key=lambda x: x[1] - x[0]):
        if lo <= n <= hi:
            return cmd

    # Fallback: extract from filename
    parts = name[6:-3].split("-")
    return parts[0] if parts else "unknown"


def get_command_source_path(cmd):
    """Map a command name to its source file path in the repo."""
    cmd_map = {
        "init": "grit/src/commands/init.rs",
        "add": "grit/src/commands/add.rs",
        "commit": "grit/src/commands/commit.rs",
        "commit/status": "grit/src/commands/commit.rs",
        "status": "grit/src/commands/status.rs",
        "branch": "grit/src/commands/branch.rs",
        "log": "grit/src/commands/log.rs",
        "config": "grit/src/commands/config.rs",
        "diff": "grit/src/commands/diff_index.rs",
        "diff-index": "grit/src/commands/diff_index.rs",
        "cat-file": "grit/src/commands/cat_file.rs",
        "hash-object": "grit/src/commands/hash_object.rs",
        "update-index": "grit/src/commands/update_index.rs",
        "ls-files": "grit/src/commands/ls_files.rs",
        "ls-tree": "grit/src/commands/ls_tree.rs",
        "write-tree": "grit/src/commands/write_tree.rs",
        "read-tree": "grit/src/commands/read_tree.rs",
        "commit-tree": "grit/src/commands/commit_tree.rs",
        "update-ref": "grit/src/commands/update_ref.rs",
        "symbolic-ref": "grit/src/commands/symbolic_ref.rs",
        "show-ref": "grit/src/commands/show_ref.rs",
        "check-ignore": "grit/src/commands/check_ignore.rs",
        "checkout-index": "grit/src/commands/checkout_index.rs",
        "checkout": "grit/src/commands/checkout_index.rs",
        "rev-parse": "grit/src/commands/rev_parse.rs",
        "rev-list": "grit/src/commands/rev_list.rs",
        "merge-base": "grit/src/commands/merge_base.rs",
        "for-each-ref": "grit/src/commands/for_each_ref.rs",
        "count-objects": "grit/src/commands/count_objects.rs",
        "verify-pack": "grit/src/commands/verify_pack.rs",
        "repack": "grit/src/commands/repack.rs",
        "gc": "grit/src/commands/gc.rs",
        "tag": "grit/src/commands/tag.rs",
        "show": "grit/src/commands/show.rs",
        "rm": "grit/src/commands/rm.rs",
        "mv": "grit/src/commands/mv.rs",
        "reset": "grit/src/commands/reset.rs",
        "restore": "grit/src/commands/restore.rs",
        "switch": "grit/src/commands/switch.rs",
        "merge": "grit/src/commands/merge.rs",
        "stash": "grit/src/commands/stash.rs",
        "fetch": "grit/src/commands/fetch.rs",
        "push": "grit/src/commands/push.rs",
        "pull": "grit/src/commands/pull.rs",
        "clone": "grit/src/commands/clone.rs",
        "remote": "grit/src/commands/remote.rs",
        "pack/prune": "grit-lib/src/pack.rs",
    }
    return cmd_map.get(cmd, "")


def generate_html():
    all_tests = get_all_upstream_tests()
    passing = get_passing_tests()
    ported = get_ported_tests()
    
    total = len(all_tests)
    num_passing = len(passing)
    num_ported = len(ported)
    pct = (num_passing / total * 100) if total else 0
    
    # Group by command
    by_command = {}
    for t in all_tests:
        cmd = guess_command(t)
        by_command.setdefault(cmd, []).append(t)
    
    # Count passing per command
    cmd_stats = {}
    for cmd, tests in by_command.items():
        cmd_passing = sum(1 for t in tests if t in passing)
        cmd_stats[cmd] = (cmd_passing, len(tests))
    
    rows = []
    for t in all_tests:
        cmd = guess_command(t)
        desc = get_test_description(GIT_TEST_DIR, t)
        is_passing = t in passing
        is_ported = t in ported
        src_path = get_command_source_path(cmd)
        
        if is_passing:
            status = "✅"
            status_class = "pass"
        elif is_ported:
            status = "🔧"
            status_class = "ported"
        else:
            status = "⬜"
            status_class = "pending"
        
        src_link = f'<a href="{SRC_BASE}/{src_path}">{cmd}</a>' if src_path else html.escape(cmd)
        test_link = f'<a href="{SRC_BASE}/git/t/{t}">{html.escape(t)}</a>'
        
        rows.append(f'''      <tr class="{status_class}">
        <td>{status}</td>
        <td>{test_link}</td>
        <td>{src_link}</td>
        <td>{html.escape(desc[:80])}</td>
      </tr>''')

    # Progress bar segments
    pct_pass = num_passing / total * 100
    pct_port = (num_ported - num_passing) / total * 100
    
    page = f'''<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Grit — Git Test Compatibility</title>
<style>
  :root {{
    --bg: #0d1117;
    --fg: #c9d1d9;
    --accent: #58a6ff;
    --green: #3fb950;
    --yellow: #d29922;
    --border: #30363d;
    --row-hover: #161b22;
    --card-bg: #161b22;
  }}
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif;
    background: var(--bg);
    color: var(--fg);
    line-height: 1.6;
    padding: 2rem;
    max-width: 1200px;
    margin: 0 auto;
  }}
  h1 {{
    font-size: 2.5rem;
    font-weight: 800;
    margin-bottom: 0.25rem;
  }}
  h1 span {{ color: var(--accent); }}
  .subtitle {{
    color: #8b949e;
    font-size: 1.1rem;
    margin-bottom: 2rem;
  }}
  .summary {{
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 1rem;
    margin-bottom: 2rem;
  }}
  .card {{
    background: var(--card-bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.25rem;
    text-align: center;
  }}
  .card .number {{
    font-size: 2.5rem;
    font-weight: 700;
    color: var(--accent);
  }}
  .card .label {{
    color: #8b949e;
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }}
  .card.green .number {{ color: var(--green); }}
  .progress-bar {{
    width: 100%;
    height: 24px;
    background: var(--border);
    border-radius: 12px;
    overflow: hidden;
    margin-bottom: 2rem;
    display: flex;
  }}
  .progress-bar .pass {{ background: var(--green); }}
  .progress-bar .ported {{ background: var(--yellow); }}
  .legend {{
    display: flex;
    gap: 1.5rem;
    margin-bottom: 1rem;
    font-size: 0.9rem;
  }}
  .legend span {{ display: flex; align-items: center; gap: 0.4rem; }}
  .legend .dot {{
    width: 12px;
    height: 12px;
    border-radius: 50%;
    display: inline-block;
  }}
  .dot.green {{ background: var(--green); }}
  .dot.yellow {{ background: var(--yellow); }}
  .dot.gray {{ background: var(--border); }}
  table {{
    width: 100%;
    border-collapse: collapse;
    font-size: 0.9rem;
  }}
  th {{
    text-align: left;
    padding: 0.75rem 0.5rem;
    border-bottom: 2px solid var(--border);
    color: #8b949e;
    font-weight: 600;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    position: sticky;
    top: 0;
    background: var(--bg);
  }}
  td {{
    padding: 0.5rem;
    border-bottom: 1px solid var(--border);
  }}
  tr:hover {{ background: var(--row-hover); }}
  tr.pass td:first-child {{ color: var(--green); }}
  tr.ported td:first-child {{ color: var(--yellow); }}
  tr.pending td:first-child {{ color: #484f58; }}
  a {{
    color: var(--accent);
    text-decoration: none;
  }}
  a:hover {{ text-decoration: underline; }}
  .filter {{
    margin-bottom: 1rem;
    display: flex;
    gap: 0.75rem;
    flex-wrap: wrap;
    align-items: center;
  }}
  .filter input {{
    background: var(--card-bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.5rem 0.75rem;
    color: var(--fg);
    font-size: 0.9rem;
    width: 300px;
  }}
  .filter input::placeholder {{ color: #484f58; }}
  .filter button {{
    background: var(--card-bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.5rem 0.75rem;
    color: var(--fg);
    cursor: pointer;
    font-size: 0.85rem;
  }}
  .filter button:hover {{ background: var(--border); }}
  .filter button.active {{ border-color: var(--accent); color: var(--accent); }}
  footer {{
    margin-top: 3rem;
    padding-top: 1rem;
    border-top: 1px solid var(--border);
    color: #484f58;
    font-size: 0.8rem;
    text-align: center;
  }}
</style>
</head>
<body>

<h1><span>Grit</span> — Git in Rust</h1>
<p class="subtitle">A from-scratch reimplementation of Git in idiomatic, library-oriented Rust.
  <a href="{GITHUB_BASE}">GitHub</a>
</p>

<div class="summary">
  <div class="card green">
    <div class="number">{num_passing}</div>
    <div class="label">Tests Passing</div>
  </div>
  <div class="card">
    <div class="number">{total}</div>
    <div class="label">Total Upstream Tests</div>
  </div>
  <div class="card green">
    <div class="number">{pct:.1f}%</div>
    <div class="label">Compatibility</div>
  </div>
  <div class="card">
    <div class="number">{len(by_command)}</div>
    <div class="label">Command Areas</div>
  </div>
</div>

<div class="progress-bar">
  <div class="pass" style="width: {pct_pass:.1f}%"></div>
  <div class="ported" style="width: {pct_port:.1f}%"></div>
</div>

<div class="legend">
  <span><span class="dot green"></span> Passing ({num_passing})</span>
  <span><span class="dot yellow"></span> Ported, not in harness ({num_ported - num_passing})</span>
  <span><span class="dot gray"></span> Not yet ported ({total - num_ported})</span>
</div>

<div class="filter">
  <input type="text" id="search" placeholder="Filter tests..." oninput="filterTable()">
  <button onclick="filterStatus('all')" class="active" id="btn-all">All ({total})</button>
  <button onclick="filterStatus('pass')" id="btn-pass">✅ Passing ({num_passing})</button>
  <button onclick="filterStatus('pending')" id="btn-pending">⬜ Pending ({total - num_passing})</button>
</div>

<table id="tests">
  <thead>
    <tr>
      <th style="width:40px">St</th>
      <th>Test Script</th>
      <th>Command</th>
      <th>Description</th>
    </tr>
  </thead>
  <tbody>
{chr(10).join(rows)}
  </tbody>
</table>

<footer>
  Generated from upstream <code>git/t/</code> ({total} scripts).
  Grit is a project by <a href="https://github.com/schacon">Scott Chacon</a>.
</footer>

<script>
let currentFilter = 'all';
function filterTable() {{
  const q = document.getElementById('search').value.toLowerCase();
  const rows = document.querySelectorAll('#tests tbody tr');
  rows.forEach(r => {{
    const text = r.textContent.toLowerCase();
    const matchText = !q || text.includes(q);
    const matchStatus = currentFilter === 'all' ||
      (currentFilter === 'pass' && r.classList.contains('pass')) ||
      (currentFilter === 'pending' && !r.classList.contains('pass'));
    r.style.display = matchText && matchStatus ? '' : 'none';
  }});
}}
function filterStatus(status) {{
  currentFilter = status;
  document.querySelectorAll('.filter button').forEach(b => b.classList.remove('active'));
  document.getElementById('btn-' + status).classList.add('active');
  filterTable();
}}
</script>

</body>
</html>'''
    
    with open(OUTPUT, "w") as f:
        f.write(page)
    
    print(f"Generated {OUTPUT}")
    print(f"  {total} tests, {num_passing} passing ({pct:.1f}%)")


if __name__ == "__main__":
    generate_html()
