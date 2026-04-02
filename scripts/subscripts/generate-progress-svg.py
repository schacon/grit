#!/usr/bin/env python3
"""Generate docs/progress.svg — a dynamic badge showing upstream test pass rate.

Reads results from /tmp/grit-upstream-results/ (populated by run-upstream-tests.sh).
Falls back to scanning existing docs/index.html for last known values.
"""
import os
import re

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RESULTS_DIR = "/tmp/grit-upstream-results"
OUTPUT = os.path.join(REPO_ROOT, "docs", "progress.svg")


def get_counts_from_results():
    """Read TAP output from upstream test results."""
    if not os.path.isdir(RESULTS_DIR):
        return None, None
    total_pass = 0
    total_fail = 0
    for f in os.listdir(RESULTS_DIR):
        if not f.endswith(".out"):
            continue
        path = os.path.join(RESULTS_DIR, f)
        try:
            with open(path, errors="replace") as fh:
                content = fh.read()
            total_pass += len(re.findall(r"^ok [0-9]", content, re.MULTILINE))
            total_fail += len(re.findall(r"^not ok [0-9]", content, re.MULTILINE))
        except Exception:
            pass
    total = total_pass + total_fail
    if total == 0:
        return None, None
    return total_pass, total


def get_counts_from_html():
    """Read values from docs/index.html to match the dashboard exactly."""
    index = os.path.join(REPO_ROOT, "docs", "index.html")
    if not os.path.isfile(index):
        return 0, 1
    try:
        with open(index) as f:
            content = f.read()
        # Match the first two "number" divs: passing count and total count
        numbers = re.findall(r'<div class="number">(\d[\d,]*)</div>', content)
        if len(numbers) >= 2:
            pass_count = int(numbers[0].replace(",", ""))
            total_count = int(numbers[1].replace(",", ""))
            return pass_count, total_count
    except Exception:
        pass
    return 0, 1


def generate_svg(pass_count, total_count):
    pct = pass_count * 100 / total_count if total_count > 0 else 0
    bar_width = 440 * pct / 100

    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="480" height="120" viewBox="0 0 480 120">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#1a1e24"/>
      <stop offset="100%" stop-color="#0d1117"/>
    </linearGradient>
  </defs>
  <rect width="480" height="120" rx="10" fill="url(#bg)" stroke="#30363d" stroke-width="1"/>
  <text x="20" y="32" font-family="-apple-system,BlinkMacSystemFont,Segoe UI,Helvetica,Arial,sans-serif" font-size="18" font-weight="700" fill="#c9d1d9">Grit — Git in Rust</text>
  <text x="20" y="58" font-family="-apple-system,BlinkMacSystemFont,Segoe UI,Helvetica,Arial,sans-serif" font-size="14" fill="#8b949e">{pass_count:,} / {total_count:,} upstream tests passing</text>
  <rect x="20" y="72" width="440" height="16" rx="8" fill="#30363d"/>
  <rect x="20" y="72" width="{bar_width:.0f}" height="16" rx="8" fill="#3fb950"/>
  <text x="20" y="106" font-family="-apple-system,BlinkMacSystemFont,Segoe UI,Helvetica,Arial,sans-serif" font-size="24" font-weight="700" fill="#3fb950">{pct:.1f}%</text>
  <text x="100" y="106" font-family="-apple-system,BlinkMacSystemFont,Segoe UI,Helvetica,Arial,sans-serif" font-size="14" fill="#8b949e">of git test suite</text>
</svg>'''


def main():
    # Always read from index.html to match the dashboard exactly
    pass_count, total_count = get_counts_from_html()
    if pass_count == 0:
        # Fall back to upstream results only if index.html has no data
        p, t = get_counts_from_results()
        if p is not None:
            pass_count, total_count = p, t

    svg = generate_svg(pass_count, total_count)
    with open(OUTPUT, "w") as f:
        f.write(svg)

    pct = pass_count * 100 / total_count if total_count else 0
    print(f"Generated {OUTPUT}: {pass_count:,}/{total_count:,} ({pct:.1f}%)")


if __name__ == "__main__":
    main()
