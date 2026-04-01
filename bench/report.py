#!/usr/bin/env python3
"""Generate docs/bench.html from hyperfine JSON results."""

import json
import os
import glob
from datetime import datetime, timezone

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RESULTS_DIR = os.path.join(REPO_ROOT, "bench", "results")
OUTPUT = os.path.join(REPO_ROOT, "docs", "bench.html")


def load_results():
    """Load all bench/*.json result files."""
    benchmarks = []
    for path in sorted(glob.glob(os.path.join(RESULTS_DIR, "*.json"))):
        with open(path) as f:
            data = json.load(f)
        name = os.path.splitext(os.path.basename(path))[0]
        results = data.get("results", [])
        if len(results) < 2:
            continue

        git_result = None
        grit_result = None
        for r in results:
            cmd = r.get("command", "")
            if "/grit " in cmd or cmd.startswith("grit "):
                grit_result = r
            else:
                git_result = r

        if not git_result or not grit_result:
            continue

        git_mean = git_result["mean"]
        grit_mean = grit_result["mean"]
        ratio = git_mean / grit_mean if grit_mean > 0 else 0

        benchmarks.append({
            "name": name,
            "git_mean": git_mean,
            "git_stddev": git_result.get("stddev", 0),
            "git_min": git_result.get("min", git_mean),
            "git_max": git_result.get("max", git_mean),
            "grit_mean": grit_mean,
            "grit_stddev": grit_result.get("stddev", 0),
            "grit_min": grit_result.get("min", grit_mean),
            "grit_max": grit_result.get("max", grit_mean),
            "ratio": ratio,
            "runs": len(grit_result.get("times", [])),
        })

    return benchmarks


def format_time(seconds):
    """Format seconds into human-readable time."""
    if seconds < 0.001:
        return f"{seconds * 1_000_000:.1f} µs"
    elif seconds < 1.0:
        return f"{seconds * 1_000:.1f} ms"
    else:
        return f"{seconds:.2f} s"


def ratio_class(ratio):
    """CSS class based on speed ratio."""
    if ratio >= 2.0:
        return "much-faster"
    elif ratio >= 1.2:
        return "faster"
    elif ratio >= 0.8:
        return "similar"
    elif ratio >= 0.5:
        return "slower"
    else:
        return "much-slower"


def ratio_label(ratio):
    if ratio >= 1.0:
        return f"{ratio:.1f}× faster"
    else:
        return f"{1/ratio:.1f}× slower"


def generate_html(benchmarks):
    # Sort by ratio descending (fastest wins first)
    benchmarks.sort(key=lambda b: -b["ratio"])

    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")

    # Compute summary stats
    total = len(benchmarks)
    faster_count = sum(1 for b in benchmarks if b["ratio"] > 1.2)
    similar_count = sum(1 for b in benchmarks if 0.8 <= b["ratio"] <= 1.2)
    slower_count = sum(1 for b in benchmarks if b["ratio"] < 0.8)
    geo_mean = 1.0
    for b in benchmarks:
        geo_mean *= b["ratio"]
    geo_mean = geo_mean ** (1.0 / total) if total > 0 else 1.0

    rows = ""
    chart_data = []
    for b in benchmarks:
        cls = ratio_class(b["ratio"])
        chart_data.append({"name": b["name"], "ratio": round(b["ratio"], 2)})
        rows += f"""
    <tr class="{cls}">
      <td class="bench-name">{b["name"]}</td>
      <td class="time">{format_time(b["git_mean"])} <span class="range">±{format_time(b["git_stddev"])}</span></td>
      <td class="time">{format_time(b["grit_mean"])} <span class="range">±{format_time(b["grit_stddev"])}</span></td>
      <td class="ratio {cls}">{ratio_label(b["ratio"])}</td>
      <td class="bar-cell">
        <div class="bar-container">
          <div class="bar bar-git" style="width: {min(100, 100 / max(b['ratio'], 0.01)):.0f}%"></div>
          <div class="bar bar-grit" style="width: {min(100, 100 * min(b['ratio'], 10) / max(b['ratio'], 1)):.0f}%"></div>
        </div>
      </td>
      <td class="runs">{b["runs"]}</td>
    </tr>"""

    html = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Grit vs Git — Benchmarks</title>
<style>
  :root {{
    --bg: #0d1117;
    --fg: #c9d1d9;
    --accent: #58a6ff;
    --green: #3fb950;
    --yellow: #d29922;
    --red: #f85149;
    --orange: #d29922;
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
    max-width: 1400px;
    margin: 0 auto;
  }}
  a {{ color: var(--accent); text-decoration: none; }}
  a:hover {{ text-decoration: underline; }}
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
  .nav {{
    margin-bottom: 1.5rem;
    font-size: 0.95rem;
  }}
  .summary {{
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
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
    font-size: 2.2rem;
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
  .card.yellow .number {{ color: var(--yellow); }}
  .card.red .number {{ color: var(--red); }}

  table {{
    width: 100%;
    border-collapse: collapse;
    font-size: 0.9rem;
    margin-bottom: 2rem;
  }}
  th {{
    text-align: left;
    padding: 0.75rem 0.5rem;
    border-bottom: 2px solid var(--border);
    color: #8b949e;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }}
  td {{
    padding: 0.6rem 0.5rem;
    border-bottom: 1px solid var(--border);
  }}
  tr:hover {{ background: var(--row-hover); }}
  .bench-name {{
    font-weight: 600;
    font-family: 'SF Mono', 'Fira Code', monospace;
  }}
  .time {{
    font-family: 'SF Mono', 'Fira Code', monospace;
    white-space: nowrap;
  }}
  .range {{
    color: #8b949e;
    font-size: 0.8em;
  }}
  .ratio {{
    font-weight: 700;
    white-space: nowrap;
  }}
  .much-faster .ratio, .much-faster.ratio {{ color: var(--green); }}
  .faster .ratio, .faster.ratio {{ color: #56d364; }}
  .similar .ratio, .similar.ratio {{ color: var(--yellow); }}
  .slower .ratio, .slower.ratio {{ color: var(--orange); }}
  .much-slower .ratio, .much-slower.ratio {{ color: var(--red); }}
  .runs {{
    color: #8b949e;
    text-align: center;
  }}

  .bar-cell {{ width: 200px; }}
  .bar-container {{
    display: flex;
    flex-direction: column;
    gap: 2px;
  }}
  .bar {{
    height: 10px;
    border-radius: 3px;
    min-width: 4px;
  }}
  .bar-git {{ background: #8b949e; }}
  .bar-grit {{ background: var(--green); }}
  .legend-bar {{
    display: flex;
    gap: 1.5rem;
    margin-bottom: 1rem;
    font-size: 0.85rem;
    color: #8b949e;
  }}
  .legend-bar span {{
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }}
  .legend-bar .dot {{
    width: 10px;
    height: 10px;
    border-radius: 2px;
    display: inline-block;
  }}
  .dot.git {{ background: #8b949e; }}
  .dot.grit {{ background: var(--green); }}

  .methodology {{
    background: var(--card-bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.5rem;
    margin-top: 1rem;
    font-size: 0.9rem;
    color: #8b949e;
  }}
  .methodology h3 {{
    color: var(--fg);
    margin-bottom: 0.5rem;
  }}
  .methodology ul {{
    padding-left: 1.5rem;
    margin-top: 0.5rem;
  }}
  .methodology li {{
    margin-bottom: 0.25rem;
  }}
  .footer {{
    margin-top: 2rem;
    padding-top: 1rem;
    border-top: 1px solid var(--border);
    color: #8b949e;
    font-size: 0.85rem;
  }}
</style>
</head>
<body>

<div class="nav"><a href="index.html">← Test Coverage</a></div>

<h1>Grit <span>vs</span> Git</h1>
<p class="subtitle">Performance benchmarks — {now}</p>

<div class="summary">
  <div class="card"><div class="number">{total}</div><div class="label">Benchmarks</div></div>
  <div class="card green"><div class="number">{faster_count}</div><div class="label">Grit Faster</div></div>
  <div class="card yellow"><div class="number">{similar_count}</div><div class="label">Similar</div></div>
  <div class="card red"><div class="number">{slower_count}</div><div class="label">Git Faster</div></div>
  <div class="card"><div class="number">{geo_mean:.1f}×</div><div class="label">Geo Mean</div></div>
</div>

<div class="legend-bar">
  <span><span class="dot git"></span> C Git</span>
  <span><span class="dot grit"></span> Grit (Rust)</span>
</div>

<table>
  <thead>
    <tr>
      <th>Benchmark</th>
      <th>C Git</th>
      <th>Grit</th>
      <th>Ratio</th>
      <th>Visual</th>
      <th>Runs</th>
    </tr>
  </thead>
  <tbody>
{rows}
  </tbody>
</table>

<div class="methodology">
  <h3>Methodology</h3>
  <ul>
    <li>All benchmarks run via <a href="https://github.com/sharkdp/hyperfine">hyperfine</a> with {2} warmup runs and ≥{10} timed runs.</li>
    <li>Each benchmark operates on an identical repo setup (same commits, files, refs).</li>
    <li>"Ratio" = C Git mean / Grit mean. Values &gt;1.0 mean Grit is faster.</li>
    <li>Geometric mean summarizes overall performance across all benchmarks.</li>
    <li>Results vary by hardware, OS, filesystem, and repo shape.</li>
  </ul>
</div>

<div class="footer">
  Generated by <code>bench/run.sh</code> + <code>bench/report.py</code>
</div>

</body>
</html>"""
    return html


def main():
    benchmarks = load_results()
    if not benchmarks:
        print("No benchmark results found in bench/results/")
        print("Run: bash bench/run.sh")
        return

    html = generate_html(benchmarks)
    with open(OUTPUT, "w") as f:
        f.write(html)
    print(f"Generated {OUTPUT}")
    print(f"  {len(benchmarks)} benchmarks")

    faster = sum(1 for b in benchmarks if b["ratio"] > 1.2)
    slower = sum(1 for b in benchmarks if b["ratio"] < 0.8)
    print(f"  {faster} faster, {slower} slower")


if __name__ == "__main__":
    main()
