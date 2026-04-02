#!/usr/bin/env python3
"""
Script 1: Extract all test cases from git/t/*.sh and run ported tests.

Produces two files:
  data/git-test-cases.tsv  — cached list of every upstream test case with subcommands
  data/test-results.tsv    — per-file ported test results + per-upstream-test status
"""

import os
import re
import subprocess
import sys
import tempfile
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor, as_completed

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
UPSTREAM = os.path.join(REPO, "git", "t")
GRIT_TESTS = os.path.join(REPO, "tests")
DATA = os.path.join(REPO, "data")
BIN = os.path.join(REPO, "target", "release", "grit")

# ── Known git subcommands ──

GIT_SUBCOMMANDS = {
    "add", "am", "annotate", "apply", "archive", "backfill", "bisect",
    "blame", "branch", "bundle", "cat-file", "check-attr", "check-ignore",
    "check-mailmap", "check-ref-format", "checkout", "checkout-index",
    "cherry", "cherry-pick", "clean", "clone", "column", "commit",
    "commit-graph", "commit-tree", "config", "count-objects", "credential",
    "credential-cache", "credential-store", "daemon", "describe", "diagnose",
    "diff", "diff-files", "diff-index", "diff-tree", "difftool",
    "fast-export", "fast-import", "fetch", "fetch-pack", "filter-branch",
    "fmt-merge-msg", "for-each-ref", "for-each-repo", "format-patch", "fsck",
    "gc", "get-tar-commit-id", "grep", "hash-object", "help", "hook",
    "http-backend", "imap-send", "index-pack", "init", "interpret-trailers",
    "log", "ls-files", "ls-remote", "ls-tree", "mailinfo", "mailsplit",
    "maintenance", "merge", "merge-base", "merge-file", "merge-index",
    "merge-one-file", "merge-tree", "mergetool", "mktag", "mktree",
    "multi-pack-index", "mv", "name-rev", "notes", "pack-objects",
    "pack-redundant", "pack-refs", "patch-id", "prune", "prune-packed",
    "pull", "push", "range-diff", "read-tree", "rebase", "receive-pack",
    "reflog", "refs", "remote", "repack", "replace", "replay",
    "request-pull", "rerere", "reset", "restore", "rev-list", "rev-parse",
    "revert", "rm", "scalar", "send-email", "send-pack", "shortlog", "show",
    "show-branch", "show-index", "show-ref", "sparse-checkout", "stash",
    "status", "stripspace", "submodule", "switch", "symbolic-ref", "tag",
    "unpack-file", "unpack-objects", "update-index", "update-ref",
    "update-server-info", "upload-archive", "upload-pack", "var",
    "verify-commit", "verify-pack", "verify-tag", "version", "whatchanged",
    "worktree", "write-tree",
}


def slugify(text):
    """Convert a test description to a slug."""
    s = text.lower().strip()
    s = re.sub(r'[^a-z0-9]+', '-', s)
    s = s.strip('-')
    if len(s) > 80:
        s = s[:80].rstrip('-')
    return s


def extract_tests_from_file(filepath):
    """Extract test_expect_success entries from a shell test file.

    Returns list of (description, body) tuples.
    """
    try:
        with open(filepath, encoding='utf-8', errors='replace') as f:
            content = f.read()
    except Exception:
        return []

    tests = []

    # Match test_expect_success [PREREQ] 'description' '..body..'
    # We need to handle multi-line bodies in single quotes.
    # Strategy: find each test_expect_success, extract the description,
    # then extract the body by finding the matching quote.

    # Find all occurrences with single-quoted descriptions
    desc_pattern = re.compile(
        r'test_expect_success\s+'
        r'(?:[A-Z_,!]+\s+)?'
        r"""'([^']*)'"""
    )

    for m in desc_pattern.finditer(content):
        desc = m.group(1)
        rest = content[m.end():]
        body = _extract_body(rest)
        tests.append((desc, body))

    # Also double-quoted descriptions
    desc_pattern2 = re.compile(
        r'test_expect_success\s+'
        r'(?:[A-Z_,!]+\s+)?'
        r'"([^"]*)"'
    )
    for m in desc_pattern2.finditer(content):
        desc = m.group(1)
        rest = content[m.end():]
        body = _extract_body(rest)
        tests.append((desc, body))

    return tests


def _extract_body(rest):
    """Extract the body string after the description match."""
    rest = rest.lstrip()
    if not rest:
        return ""

    quote_char = rest[0]
    if quote_char == "'":
        # Single-quoted: find next unescaped single quote
        # In shell, single quotes can't contain single quotes at all
        end = rest.find("'", 1)
        if end >= 0:
            return rest[1:end]
    elif quote_char == '"':
        # Double-quoted: find matching close (skip escaped quotes)
        i = 1
        while i < len(rest):
            if rest[i] == '\\':
                i += 2
                continue
            if rest[i] == '"':
                return rest[1:i]
            i += 1

    # Fallback: grab context
    return rest[:500]


def extract_subcommands(body):
    """Find git subcommands invoked in a test body."""
    found = set()
    for m in re.finditer(r'\bgit\s+(?:(?:-[a-zA-Z][\w-]*(?:[\s=][^\s]*)?)\s+)*([a-z][\w-]*)', body):
        cmd = m.group(1)
        if cmd in GIT_SUBCOMMANDS:
            found.add(cmd)
    return sorted(found)


def generate_test_cases():
    """Parse all upstream test files and write data/git-test-cases.tsv."""
    outpath = os.path.join(DATA, "git-test-cases.tsv")
    files = sorted(f for f in os.listdir(UPSTREAM)
                   if re.match(r't\d+.*\.sh$', f))

    total = 0
    with open(outpath, 'w') as out:
        out.write("test_id\tfile\tdescription\tsubcommands\n")
        for fname in files:
            filepath = os.path.join(UPSTREAM, fname)
            base = fname.replace('.sh', '')
            tests = extract_tests_from_file(filepath)
            seen_slugs = {}
            for desc, body in tests:
                slug = slugify(desc)
                if slug in seen_slugs:
                    seen_slugs[slug] += 1
                    slug = f"{slug}-{seen_slugs[slug]}"
                else:
                    seen_slugs[slug] = 1
                test_id = f"{base}::{slug}"
                subcmds = extract_subcommands(body)
                out.write(f"{test_id}\t{base}\t{desc}\t{','.join(subcmds)}\n")
                total += 1

    print(f"Extracted {total} test cases from {len(files)} upstream files → {outpath}")
    return total


def run_ported_tests():
    """Run each ported test file with verbose output and collect per-file results.

    Returns:
      file_stats: { file_base: { 'total': int, 'pass': int, 'fail': int } }
    """
    if not os.path.isfile(BIN):
        print(f"ERROR: grit binary not found at {BIN}", file=sys.stderr)
        print("Run: cargo build --release", file=sys.stderr)
        sys.exit(1)

    test_files = sorted(f for f in os.listdir(GRIT_TESTS)
                        if re.match(r't\d+.*\.sh$', f))

    file_stats = {}

    def run_one(fname):
        """Run a single test file and return (base, total, pass, fail)."""
        base = fname.replace('.sh', '')
        trash = tempfile.mkdtemp(prefix=f"grit-test-{base}-")
        try:
            env = os.environ.copy()
            env['GUST_BIN'] = BIN
            env['TRASH_DIRECTORY'] = trash
            env['TEST_VERBOSE'] = '1'
            env['TERM'] = 'dumb'
            proc = subprocess.run(
                ['sh', f'./{fname}'],
                cwd=GRIT_TESTS,
                capture_output=True,
                text=True,
                timeout=120,
                env=env,
            )
            output = proc.stdout + proc.stderr
        except subprocess.TimeoutExpired:
            return (base, 0, 0, 0, 'timeout')
        except Exception:
            return (base, 0, 0, 0, 'error')
        finally:
            subprocess.run(['rm', '-rf', trash], capture_output=True)

        # Parse summary line: # Tests: N  Pass: N  Fail: N  Skip: N
        total = pass_n = fail_n = 0
        for line in output.split('\n'):
            m = re.match(r'# Tests:\s*(\d+)\s+Pass:\s*(\d+)\s+Fail:\s*(\d+)', line.strip())
            if m:
                total = int(m.group(1))
                pass_n = int(m.group(2))
                fail_n = int(m.group(3))

        return (base, total, pass_n, fail_n, 'ok')

    print(f"Running {len(test_files)} ported test files...")
    with ThreadPoolExecutor(max_workers=16) as pool:
        futures = {pool.submit(run_one, f): f for f in test_files}
        done = 0
        for future in as_completed(futures):
            done += 1
            if done % 50 == 0 or done == len(test_files):
                print(f"  {done}/{len(test_files)} files completed")
            try:
                base, total, pass_n, fail_n, status = future.result()
                file_stats[base] = {
                    'total': total, 'pass': pass_n, 'fail': fail_n, 'status': status
                }
            except Exception as e:
                print(f"  Error in {futures[future]}: {e}", file=sys.stderr)

    return file_stats


def generate_test_results(file_stats):
    """Read git-test-cases.tsv and write test-results.tsv with per-test status.

    For each upstream test case:
    - If the file has been ported (exists in tests/) AND all tests pass → "pass"
    - If the file has been ported but some fail → "partial"
    - If the file has not been ported → "not_ported"
    """
    cases_path = os.path.join(DATA, "git-test-cases.tsv")
    results_path = os.path.join(DATA, "test-results.tsv")
    file_results_path = os.path.join(DATA, "file-results.tsv")

    # Also check which files exist in tests/ (even if they had errors)
    ported_files = set()
    for f in os.listdir(GRIT_TESTS):
        if re.match(r't\d+.*\.sh$', f):
            ported_files.add(f.replace('.sh', ''))

    # Write per-file results
    total_ported_pass = 0
    total_ported_tests = 0
    with open(file_results_path, 'w') as out:
        out.write("file\tported\ttotal_tests\tpassing\tfailing\tstatus\n")
        all_files = set()
        with open(cases_path) as inp:
            inp.readline()
            for line in inp:
                parts = line.rstrip('\n').split('\t')
                if len(parts) >= 2:
                    all_files.add(parts[1])

        for fbase in sorted(all_files):
            ported = fbase in ported_files
            if ported and fbase in file_stats:
                s = file_stats[fbase]
                total_ported_tests += s['total']
                total_ported_pass += s['pass']
                out.write(f"{fbase}\tyes\t{s['total']}\t{s['pass']}\t{s['fail']}\t{s['status']}\n")
            elif ported:
                out.write(f"{fbase}\tyes\t0\t0\t0\tunknown\n")
            else:
                out.write(f"{fbase}\tno\t0\t0\t0\tnot_ported\n")

    # Write per-test-case results
    pass_count = 0
    partial_count = 0
    not_ported_count = 0

    with open(cases_path) as inp, open(results_path, 'w') as out:
        header = inp.readline()
        out.write("test_id\tfile\tdescription\tsubcommands\tstatus\n")
        for line in inp:
            parts = line.rstrip('\n').split('\t')
            if len(parts) < 4:
                continue
            test_id, fbase, desc, subcmds = parts[0], parts[1], parts[2], parts[3]

            if fbase in ported_files and fbase in file_stats:
                s = file_stats[fbase]
                if s['total'] > 0 and s['fail'] == 0:
                    status = "pass"
                    pass_count += 1
                elif s['total'] > 0:
                    status = "partial"
                    partial_count += 1
                else:
                    status = "not_ported"
                    not_ported_count += 1
            else:
                status = "not_ported"
                not_ported_count += 1

            out.write(f"{test_id}\t{fbase}\t{desc}\t{subcmds}\t{status}\n")

    print(f"\nPer upstream test case: {pass_count} pass (file fully passing), "
          f"{partial_count} partial, {not_ported_count} not ported")
    print(f"Ported test suite: {total_ported_pass:,} / {total_ported_tests:,} individual tests passing")
    print(f"Wrote {results_path}")
    print(f"Wrote {file_results_path}")


def main():
    os.makedirs(DATA, exist_ok=True)

    # Step 1: extract all upstream test cases (cached)
    cases_path = os.path.join(DATA, "git-test-cases.tsv")
    if os.path.exists(cases_path) and '--force' not in sys.argv:
        print(f"Using cached {cases_path} (use --force to regenerate)")
    else:
        generate_test_cases()

    # Step 2: run ported tests and generate results
    file_stats = run_ported_tests()
    generate_test_results(file_stats)


if __name__ == '__main__':
    main()
