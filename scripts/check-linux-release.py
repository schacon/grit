#!/usr/bin/env python3
"""Check the x86_64 GNU release's loader contract without changing ASLR settings."""
import argparse
import json
import os
from pathlib import Path
import re
import select
import subprocess
import tempfile


def readelf(binary, option):
    return subprocess.check_output(
        ["readelf", option, str(binary)], text=True, env={**os.environ, "LC_ALL": "C"}
    )


def check(binary, max_glibc, relr):
    header = readelf(binary, "-hW")
    segments = readelf(binary, "-lW")
    dynamic = readelf(binary, "-dW")
    versions = readelf(binary, "-VW")
    assert re.search(r"Type:\s+DYN", header), "release must be PIE"
    assert "Advanced Micro Devices X86-64" in header, "only x86_64 GNU is verified"
    assert re.search(r"\(FLAGS_1\).*\bPIE\b", dynamic), "missing PIE dynamic flag"
    assert "GNU_RELRO" in segments, "missing GNU_RELRO"
    assert "BIND_NOW" in dynamic, "missing BIND_NOW"
    stack = next(line for line in segments.splitlines() if "GNU_STACK" in line)
    assert "RWE" not in stack, "executable stack"
    interpreter = re.search(r"Requesting program interpreter: ([^\]]+)", segments)
    assert interpreter and interpreter[1] == "/lib64/ld-linux-x86-64.so.2", "unexpected interpreter"
    needed = re.findall(r"\(NEEDED\).*\[([^]]+)\]", dynamic)
    assert "libc.so.6" in needed, "missing GNU libc dependency"
    assert set(needed) <= {"ld-linux-x86-64.so.2", "libc.so.6", "libgcc_s.so.1", "libm.so.6", "libpthread.so.0", "libdl.so.2", "librt.so.1"}, "unexpected dependency"
    assert not re.search(r"\((?:RPATH|RUNPATH|TEXTREL)\)", dynamic), "unexpected search path or text relocation"
    packed = "(RELR)" in dynamic
    assert packed == (relr == "required"), f"RELR must be {relr}"
    assert ("GLIBC_ABI_DT_RELR" in versions) == packed, "missing or unexpected RELR loader ABI requirement"
    if packed:
        assert "(RELRSZ)" in dynamic and "(RELRENT)" in dynamic, "incomplete RELR metadata"
        assert max_glibc >= (2, 36), "RELR needs glibc 2.36 or later"
    # A version needed only by weak symbols is not a loader minimum. In particular,
    # Rust probes glibc 2.39's pidfd_spawnp and falls back on older loaders.
    strong_versions = []
    for version, flags in re.findall(r"Name: GLIBC_([0-9.]+)\s+Flags: (\w+)", versions):
        if flags != "WEAK":
            parsed = tuple(map(int, version.split(".")))
            assert parsed <= max_glibc, f"strong GLIBC_{version} exceeds loader baseline"
            strong_versions.append(version)
    assert strong_versions, "no GNU symbol version requirements found"
    return {"binary": str(binary), "interpreter": interpreter[1], "needed": needed,
            "strong_glibc_versions": strong_versions, "relr": packed}


def runtime(binary):
    subprocess.run([str(binary), "--help"], stdout=subprocess.DEVNULL, check=True, timeout=10)
    bases = []
    with tempfile.TemporaryDirectory(prefix="grit-release-check-") as directory:
        subprocess.run(["git", "init", "--bare", "-q", directory], check=True)
        for _ in range(4):
            # upload-pack advertises refs, then waits for input. Inspect that actual
            # grit process while blocked on stdin, with the normal loader/ASLR.
            with subprocess.Popen([str(binary), "upload-pack", directory],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE) as child:
                try:
                    assert select.select([child.stdout], [], [], 10)[0], "startup timed out"
                    assert os.read(child.stdout.fileno(), 4), "upload-pack failed at startup"
                    mappings = Path(f"/proc/{child.pid}/maps").read_text().splitlines()
                    mapping = next(line for line in mappings if line.endswith(str(binary)))
                    bases.append(mapping.split("-", 1)[0])
                finally:
                    child.kill()
                    child.wait()
    assert len(set(bases)) == len(bases), "PIE load addresses did not randomize"
    return bases


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--max-glibc", required=True)
    parser.add_argument("--relr", choices=["required", "forbidden"], required=True)
    parser.add_argument("--run", action="store_true", help="check startup and runtime address randomization")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    result = check(binary, tuple(map(int, args.max_glibc.split("."))), args.relr)
    if args.run:
        result["randomized_load_addresses"] = runtime(binary)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
