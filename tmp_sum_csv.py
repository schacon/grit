#!/usr/bin/env python3
import csv
from pathlib import Path

DATA = Path(__file__).resolve().parent / "data" / "test-files.csv"
total_tests = total_pass = 0
groups: dict[str, dict[str, int]] = {}
with DATA.open(newline="", encoding="utf-8") as f:
    for row in csv.DictReader(f, delimiter="\t"):
        if row.get("in_scope", "yes").strip().lower() == "skip":
            continue
        try:
            tt = int(row.get("tests_total") or 0)
            pl = int(row.get("passed_last") or 0)
        except ValueError:
            continue
        total_tests += tt
        total_pass += pl
        g = row.get("group") or "t?"
        if g not in groups:
            groups[g] = {"tests": 0, "pass": 0, "files": 0, "full": 0}
        groups[g]["tests"] += tt
        groups[g]["pass"] += pl
        groups[g]["files"] += 1
        if (row.get("fully_passing") or "").lower() == "true" and tt > 0:
            groups[g]["full"] += 1

print("total_tests", total_tests)
print("total_pass", total_pass)
print("pct", round(100.0 * total_pass / total_tests, 1) if total_tests else 0)
st = groups["t5"]
pc = round(100.0 * st["pass"] / st["tests"], 1) if st["tests"] else 0
print("t5", st["full"], st["files"], st["pass"], st["tests"], pc)
