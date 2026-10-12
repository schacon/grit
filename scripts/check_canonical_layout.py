#!/usr/bin/env python3
"""Verify every grit_lib module used outside grit-lib/src has a canonical path entry."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MAP_FILE = ROOT / "grit-lib" / "canonical-modules.toml"


def external_modules() -> set[str]:
    cmd = [
        "rg",
        "-o",
        r"grit_lib::[a-z_][a-z0-9_]*",
        str(ROOT),
        "--glob",
        "*.rs",
        "--glob",
        "!grit-lib/src/**",
    ]
    out = subprocess.check_output(cmd, text=True)
    mods: set[str] = set()
    for line in out.splitlines():
        mods.add(line.rsplit("grit_lib::", 1)[-1])
    return mods


def parse_map() -> dict[str, str]:
    text = MAP_FILE.read_text(encoding="utf-8")
    in_map = False
    mapping: dict[str, str] = {}
    for line in text.splitlines():
        if line.strip() == "[map]":
            in_map = True
            continue
        if not in_map or not line.strip() or line.startswith("#"):
            continue
        if line.startswith("["):
            break
        m = re.match(r"^([a-z_][a-z0-9_]*) = \"([^\"]*)\"$", line.strip())
        if not m:
            raise ValueError(f"bad map line: {line!r}")
        mapping[m.group(1)] = m.group(2)
    return mapping


def main() -> int:
    used = external_modules()
    mapping = parse_map()
    missing = sorted(used - mapping.keys())
    extra = sorted(set(mapping.keys()) - used)
    if missing:
        print("Missing canonical map entries for externally used modules:", file=sys.stderr)
        for m in missing:
            print(f"  {m}", file=sys.stderr)
    if extra:
        print("Note: map entries not referenced outside grit-lib/src:", len(extra))
    return 1 if missing else 0


if __name__ == "__main__":
    raise SystemExit(main())
