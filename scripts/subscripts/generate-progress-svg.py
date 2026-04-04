#!/usr/bin/env python3
"""Generate docs/progress.svg — a dynamic badge showing upstream test pass rate.

Reads from data/command-status.tsv (same source as docs/index.html) so the
badge, dashboard, and per-command bars all report the same numbers.
"""
import os

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DATA = os.path.join(REPO_ROOT, "data")
OUTPUT = os.path.join(REPO_ROOT, "docs", "progress.svg")


def get_counts():
    """Sum passing and total tests from command-status.tsv."""
    path = os.path.join(DATA, "command-status.tsv")
    total = 0
    passing = 0
    with open(path) as f:
        f.readline()  # header
        for line in f:
            parts = line.strip().split('\t')
            if len(parts) < 5:
                continue
            total += int(parts[3])
            passing += int(parts[4])
    return passing, total


def generate_svg(pass_count, total_count):
    pct = pass_count * 100 / total_count if total_count > 0 else 0
    bar_total = 432
    bar_fill = bar_total * pct / 100

    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="480" height="180" viewBox="0 0 480 180">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#1a1e24"/>
      <stop offset="100%" stop-color="#0d1117"/>
    </linearGradient>
  </defs>
  <rect width="480" height="180" rx="10" fill="url(#bg)" stroke="#30363d" stroke-width="1"/>
  <text x="24" y="40" font-family="-apple-system,BlinkMacSystemFont,Segoe UI,Helvetica,Arial,sans-serif" font-size="18" font-weight="700" fill="#c9d1d9">Grit \u2014 Git in Rust</text>
  <text x="24" y="72" font-family="-apple-system,BlinkMacSystemFont,Segoe UI,Helvetica,Arial,sans-serif" font-size="14" fill="#8b949e">{pass_count:,} / {total_count:,} upstream tests passing</text>
  <rect x="24" y="92" width="{bar_total}" height="16" rx="8" fill="#30363d"/>
  <rect x="24" y="92" width="{bar_fill:.0f}" height="16" rx="8" fill="#3fb950"/>
  <text x="24" y="138" font-family="-apple-system,BlinkMacSystemFont,Segoe UI,Helvetica,Arial,sans-serif" font-size="24" font-weight="700" fill="#3fb950">{pct:.1f}%</text>
  <text x="104" y="138" font-family="-apple-system,BlinkMacSystemFont,Segoe UI,Helvetica,Arial,sans-serif" font-size="14" fill="#8b949e">of git test suite</text>
</svg>'''


def main():
    pass_count, total_count = get_counts()
    svg = generate_svg(pass_count, total_count)
    with open(OUTPUT, "w") as f:
        f.write(svg)

    pct = pass_count * 100 / total_count if total_count else 0
    print(f"Generated {OUTPUT}: {pass_count:,}/{total_count:,} ({pct:.1f}%)")


if __name__ == "__main__":
    main()
