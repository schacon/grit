#!/usr/bin/env python3
"""Build or merge data/test-files.csv from tests/t*.sh.

Scans tests/ for harness files, assigns group (t0..t9 from the first digit after t),
counts test_expect_success / test_expect_failure per file, and merges with any
existing CSV so run results are preserved for files that still exist.
"""

from __future__ import annotations

import csv
import os
import re
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TESTS_DIR = REPO / "tests"
DATA_DIR = REPO / "data"
OUT = DATA_DIR / "test-files.csv"

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

FILE_RE = re.compile(r"^t\d+.+\.sh$")


def count_expects_and_group(sh_path: Path) -> tuple[str, int, int]:
    """Return (group, test markers count, test_expect_failure count)."""
    name = sh_path.name
    m = re.match(r"^t(\d)", name)
    group = f"t{m.group(1)}" if m else "t?"
    try:
        text = sh_path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return group, 0, 0
    markers = len(
        re.findall(r"\btest_expect_success\b|\btest_expect_failure\b", text)
    )
    ef = len(re.findall(r"\btest_expect_failure\b", text))
    return group, markers, ef


def load_existing() -> dict[str, dict[str, str]]:
    if not OUT.exists():
        return {}
    rows: dict[str, dict[str, str]] = {}
    with OUT.open(newline="", encoding="utf-8") as f:
        reader = csv.DictReader(f, delimiter="\t")
        for row in reader:
            key = row.get("file", "").strip()
            if key:
                rows[key] = row
    return rows


def main() -> None:
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    existing = load_existing()
    discovered: list[str] = []
    for p in sorted(TESTS_DIR.glob("t*.sh")):
        if FILE_RE.match(p.name):
            discovered.append(p.stem)

    merged: dict[str, dict[str, str]] = {}

    for base in discovered:
        sh = TESTS_DIR / f"{base}.sh"
        group, tests_total, expect_failure = count_expects_and_group(sh)
        prev = existing.get(base, {})
        if prev and prev.get("file"):
            in_scope = prev.get("in_scope", "yes")
            passed_last = prev.get("passed_last", "0")
            failing = prev.get("failing", "0")
            fully_passing = prev.get("fully_passing", "false")
            status = prev.get("status", "")
        else:
            in_scope = "yes"
            passed_last = "0"
            failing = "0"
            fully_passing = "false"
            status = ""

        merged[base] = {
            "file": base,
            "group": group,
            "in_scope": in_scope,
            "tests_total": str(tests_total),
            "passed_last": passed_last,
            "failing": failing,
            "fully_passing": fully_passing,
            "status": status,
            "expect_failure": str(expect_failure),
        }

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with OUT.open("w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=HEADER, delimiter="\t", lineterminator="\n")
        w.writeheader()
        for base in sorted(merged.keys()):
            w.writerow(merged[base])

    print(f"Wrote {OUT} ({len(merged)} files)")


if __name__ == "__main__":
    main()
