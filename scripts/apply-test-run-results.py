#!/usr/bin/env python3
"""Merge a batch of harness run results into data/test-files.csv and refresh dashboards."""

from __future__ import annotations

import argparse
import csv
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CSV_PATH = REPO / "data" / "test-files.csv"
GEN_DASH = REPO / "scripts" / "generate-dashboard-from-test-files.py"

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


def load_csv() -> list[dict[str, str]]:
    if not CSV_PATH.exists():
        print(f"ERROR: {CSV_PATH} missing. Run: python3 scripts/generate-test-files-catalog.py", file=sys.stderr)
        sys.exit(1)
    rows: list[dict[str, str]] = []
    with CSV_PATH.open(newline="", encoding="utf-8") as f:
        reader = csv.DictReader(f, delimiter="\t")
        for row in reader:
            rows.append(dict(row))
    return rows


def parse_run_file(path: Path) -> dict[str, tuple[int, int, int, str, int]]:
    """file -> (total, pass, fail, status, expect_failure)."""
    out: dict[str, tuple[int, int, int, str, int]] = {}
    with path.open(encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            parts = line.split("\t")
            if len(parts) < 6:
                continue
            base, total_s, pass_s, fail_s, status, ef_s = parts[:6]
            out[base] = (
                int(total_s),
                int(pass_s),
                int(fail_s),
                status,
                int(ef_s),
            )
    return out


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "run_path",
        type=Path,
        help="TSV lines: file_base, tests_total, passed, failing, status, expect_failure",
    )
    parser.add_argument(
        "--skip-dashboard",
        action="store_true",
        help="Update CSV only; skip docs dashboards (index.html, testfiles.html, test-progress.svg; caller may run generate-dashboard-from-test-files.py once at the end).",
    )
    args = parser.parse_args()
    run_path = args.run_path
    if not run_path.is_file():
        print(f"ERROR: {run_path} not found", file=sys.stderr)
        sys.exit(1)

    updates = parse_run_file(run_path)
    rows = load_csv()
    by_file = {r["file"]: i for i, r in enumerate(rows) if r.get("file")}

    for base, (total, pass_n, fail_n, status, ef) in updates.items():
        if base not in by_file:
            print(f"WARNING: unknown file {base!r} (not in catalog); skipping", file=sys.stderr)
            continue
        i = by_file[base]
        row = rows[i]
        row["tests_total"] = str(total)
        row["passed_last"] = str(pass_n)
        row["failing"] = str(fail_n)
        row["status"] = status
        row["expect_failure"] = str(ef)
        fully = "true" if total > 0 and fail_n == 0 else "false"
        row["fully_passing"] = fully

    CSV_PATH.parent.mkdir(parents=True, exist_ok=True)
    with CSV_PATH.open("w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=HEADER, delimiter="\t", lineterminator="\n")
        w.writeheader()
        for row in rows:
            w.writerow({k: row.get(k, "") for k in HEADER})

    if not args.skip_dashboard:
        subprocess.run(
            [sys.executable, str(GEN_DASH)],
            cwd=str(REPO),
            check=True,
        )
        print(
            f"Updated {CSV_PATH} and regenerated docs/index.html, docs/testfiles.html, docs/test-progress.svg"
        )


if __name__ == "__main__":
    main()
