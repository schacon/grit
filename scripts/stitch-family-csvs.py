#!/usr/bin/env python3
"""Merge per-family harness CSVs from data/family/<digit>.csv into data/test-files.csv.

Each family file is a full catalog snapshot with rows for that family's tests updated.
For every row, if a family CSV exists for that row's ``group`` (``t0``–``t9``), the row
from that file is used; otherwise the row from the baseline CSV is kept. Row order
follows the baseline file so no entries are dropped or reordered.
"""

from __future__ import annotations

import argparse
import csv
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DEFAULT_BASE = REPO / "data" / "test-files.csv"
DEFAULT_FAMILY_DIR = REPO / "data" / "family"

HEADER = [
    "file",
    "group",
    "in_scope",
    "tests_total",
    "passed_last",
    "failing",
    "fully_passing",
    "status",
    "expect_failure",
]


def digit_from_group(group: str) -> str:
    """First digit after ``t`` (``t3`` -> ``3``)."""
    g = (group or "").strip()
    if len(g) >= 2 and g[0] == "t" and g[1].isdigit():
        return g[1]
    return ""


def load_indexed(path: Path) -> dict[str, dict[str, str]]:
    rows: dict[str, dict[str, str]] = {}
    with path.open(newline="", encoding="utf-8") as f:
        reader = csv.DictReader(f, delimiter="\t")
        for row in reader:
            fn = row.get("file", "").strip()
            if fn:
                rows[fn] = dict(row)
    return rows


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--base",
        type=Path,
        default=DEFAULT_BASE,
        help=f"Baseline CSV (default: {DEFAULT_BASE}).",
    )
    parser.add_argument(
        "--family-dir",
        type=Path,
        default=DEFAULT_FAMILY_DIR,
        help=f"Directory containing <digit>.csv (default: {DEFAULT_FAMILY_DIR}).",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=None,
        help="Output CSV (default: same as --base).",
    )
    args = parser.parse_args()
    base_path = args.base
    out_path = args.out if args.out is not None else base_path
    family_dir = args.family_dir

    if not base_path.is_file():
        print(f"ERROR: baseline CSV missing: {base_path}", file=sys.stderr)
        sys.exit(1)

    baseline_rows: list[dict[str, str]] = []
    with base_path.open(newline="", encoding="utf-8") as f:
        reader = csv.DictReader(f, delimiter="\t")
        for row in reader:
            baseline_rows.append(dict(row))

    family_maps: dict[str, dict[str, dict[str, str]]] = {}
    if family_dir.is_dir():
        for p in sorted(family_dir.glob("*.csv")):
            stem = p.stem
            if stem.isdigit():
                family_maps[stem] = load_indexed(p)

    stitched: list[dict[str, str]] = []
    for row in baseline_rows:
        fn = row.get("file", "").strip()
        group = row.get("group", "")
        d = digit_from_group(group)
        if d and d in family_maps and fn in family_maps[d]:
            stitched.append(family_maps[d][fn])
        else:
            stitched.append(row)

    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=HEADER, delimiter="\t", lineterminator="\n")
        w.writeheader()
        for row in stitched:
            w.writerow({k: row.get(k, "") for k in HEADER})

    print(f"Wrote {out_path} ({len(stitched)} rows)")


if __name__ == "__main__":
    main()
