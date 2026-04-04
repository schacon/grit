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
    "credential-cache", "credential-store", "cvsimport", "cvsserver",
    "daemon", "describe", "diagnose",
    "diff", "diff-files", "diff-index", "diff-tree", "difftool",
    "fast-export", "fast-import", "fetch", "fetch-pack", "filter-branch",
    "fmt-merge-msg", "for-each-ref", "for-each-repo", "format-patch", "fsck",
    "gc", "get-tar-commit-id", "gitweb", "grep", "hash-object", "help",
    "hook", "http-backend", "imap-send", "index-pack", "init",
    "interpret-trailers", "log", "ls-files", "ls-remote", "ls-tree",
    "mailinfo", "mailsplit", "maintenance", "merge", "merge-base",
    "merge-file", "merge-index", "merge-one-file", "merge-tree", "mergetool",
    "mktag", "mktree", "multi-pack-index", "mv", "name-rev", "notes",
    "p4", "pack-objects", "pack-redundant", "pack-refs", "patch-id", "prune",
    "prune-packed", "pull", "push", "range-diff", "read-tree", "rebase",
    "receive-pack", "reflog", "refs", "remote", "repack", "replace",
    "replay", "request-pull", "rerere", "reset", "restore", "rev-list",
    "rev-parse", "revert", "rm", "scalar", "send-email", "send-pack",
    "shortlog", "show", "show-branch", "show-index", "show-ref",
    "sparse-checkout", "stash", "status", "stripspace", "submodule", "svn",
    "switch", "symbolic-ref", "tag", "unpack-file", "unpack-objects",
    "update-index", "update-ref", "update-server-info", "upload-archive",
    "upload-pack", "var", "verify-commit", "verify-pack", "verify-tag",
    "version", "whatchanged", "worktree", "write-tree",
}

# ── File name overrides ──
# Maps test file basenames to a subcommand when the filename doesn't
# directly encode a known subcommand and body-scanning can't find one.

FILE_COMMAND_OVERRIDES = {
    # Infrastructure / core
    "t0000-basic": "init",
    "t0002-gitfile": "init",
    "t0003-attributes": "check-attr",
    "t0004-unwritable": "init",
    "t0005-signals": "init",
    "t0006-date": "log",
    "t0008-ignores": "check-ignore",
    "t0013-sha1dc": "hash-object",
    "t0014-alias": "config",
    "t0017-env-helper": "var",
    "t0018-advice": "config",
    "t0019-json-writer": "init",
    "t0020-crlf": "config",
    "t0021-conversion": "config",
    "t0027-auto-crlf": "config",
    "t0029-core-unsetenvvars": "config",
    "t0033-safe-directory": "config",
    "t0034-root-safe-directory": "config",
    "t0035-safe-bare-repository": "config",
    "t0040-parse-options": "init",
    "t0041-usage": "help",
    "t0052-simple-ipc": "init",
    "t0056-git-C": "rev-parse",
    "t0060-path-utils": "init",
    "t0061-run-command": "init",
    "t0062-revision-walking": "rev-list",
    "t0066-dir-iterator": "init",
    "t0067-parse_pathspec_file": "init",
    "t0070-fundamental": "init",
    "t0071-sort": "init",
    "t0080-unit-test-output": "init",
    "t0081-find-pack": "pack-objects",
    "t0090-cache-tree": "read-tree",
    "t0091-bugreport": "diagnose",
    "t0095-bloom": "commit-graph",
    "t0101-at-syntax": "rev-parse",
    "t0200-gettext-basic": "init",
    "t0201-gettext-fallbacks": "init",
    "t0202-gettext-perl": "init",
    "t0204-gettext-reencode-sanity": "init",
    "t0210-trace2-normal": "init",
    "t0211-trace2-perf": "init",
    "t0212-trace2-event": "init",
    "t0213-trace2-ancestry": "init",
    "t0300-credentials": "credential",
    "t0450-txt-doc-vs-help": "help",
    "t0500-progress-display": "init",
    "t0600-reffiles-backend": "refs",
    # Large file handling
    "t1050-large": "add",
    "t1051-large-conversion": "add",
    "t1060-object-corruption": "fsck",
    # Config / setup
    "t1304-default-acl": "init",
    "t1306-xdg-files": "config",
    "t1309-early-config": "config",
    "t1405-main-ref-store": "refs",
    "t1419-exclude-refs": "rev-list",
    "t1501-work-tree": "init",
    "t1509-root-work-tree": "init",
    "t1510-repo-setup": "init",
    "t1517-outside-repo": "rev-parse",
    "t1600-index": "update-index",
    "t1700-split-index": "update-index",
    "t1900-repo-info": "rev-parse",
    "t2050-git-dir-relative": "init",
    "t2081-parallel-checkout-collisions": "checkout",
    "t2501-cwd-empty": "init",
    # Matching / naming
    "t3070-wildmatch": "ls-files",
    "t3211-peel-ref": "show-ref",
    "t3300-funny-names": "ls-files",
    "t3450-history": "log",
    "t3900-i18n-commit": "commit",
    "t3902-quoted": "ls-files",
    "t3910-mac-os-precompose": "init",
    "t3920-crlf-messages": "commit",
    # Diff-related
    "t4007-rename-3": "diff",
    "t4026-color": "config",
    "t4052-stat-output": "diff",
    "t4203-mailmap": "check-mailmap",
    "t4211-line-log": "log",
    # Pack / archive
    "t5000-tar-tree": "archive",
    "t5300-pack-object": "pack-objects",
    "t5302-pack-index": "index-pack",
    "t5303-pack-corruption-resilience": "pack-objects",
    "t5308-pack-detect-duplicates": "pack-objects",
    "t5310-pack-bitmaps": "pack-objects",
    "t5313-pack-bounds-checks": "pack-objects",
    "t5320-delta-islands": "pack-objects",
    "t5324-split-commit-graph": "commit-graph",
    "t5325-reverse-index": "index-pack",
    "t5332-multi-pack-reuse": "multi-pack-index",
    "t5333-pseudo-merge-bitmaps": "multi-pack-index",
    "t5334-incremental-multi-pack-index": "multi-pack-index",
    "t5351-unpack-large-objects": "unpack-objects",
    # Hooks
    "t5401-update-hooks": "receive-pack",
    "t5402-post-merge-hook": "merge",
    # Transport / fetch / push
    "t5503-tagfollow": "fetch",
    "t5511-refspec": "fetch",
    "t5540-http-push-webdav": "push",
    "t5541-http-push-smart": "push",
    "t5546-receive-limits": "receive-pack",
    "t5550-http-fetch-dumb": "fetch",
    "t5551-http-fetch-smart": "fetch",
    "t5555-http-smart-common": "fetch",
    "t5557-http-get": "fetch",
    "t5563-simple-http-auth": "fetch",
    "t5570-git-daemon": "daemon",
    "t5615-alternate-env": "init",
    "t5616-partial-clone": "clone",
    "t5700-protocol-v1": "fetch",
    "t5701-git-serve": "upload-pack",
    "t5702-protocol-v2": "fetch",
    "t5810-proto-disable-local": "fetch",
    "t5812-proto-disable-http": "fetch",
    "t5900-repo-selection": "init",
    # Internals
    "t6114-keep-packs": "repack",
    "t6130-pathspec-noglob": "ls-files",
    "t6131-pathspec-icase": "ls-files",
    "t6404-recursive-merge": "merge",
    "t6501-freshen-objects": "gc",
    "t6600-test-reach": "commit-graph",
    "t6700-tree-depth": "read-tree",
    # UI / pager
    "t7006-pager": "log",
    "t7010-setup": "init",
    "t7450-bad-git-dotfiles": "init",
    "t7505-prepare-commit-msg-hook": "commit",
    "t7527-builtin-fsmonitor": "status",
    "t8020-last-modified": "status",
    # SVN edge cases
    "t9150-svk-mergetickets": "svn",
    "t9151-svn-mergeinfo": "svn",
    "t9152-svn-empty-dirs-after-gc": "svn",
    # Completion / prompt / shell
    "t9700-perl-git": "init",
    "t9850-shell": "help",
    "t9902-completion": "help",
    "t9903-bash-prompt": "status",
    # P4 edge cases
    "t9832-unshelve": "p4",
    "t9833-errors": "p4",
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
        desc = m.group(1).replace('\n', ' ').replace('\t', ' ').strip()
        if not desc:
            continue
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
        desc = m.group(1).replace('\n', ' ').replace('\t', ' ').strip()
        if not desc:
            continue
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


def file_theme_command(file_base):
    """Infer the primary git subcommand from a test filename like t3200-branch."""
    # Check explicit overrides first
    if file_base in FILE_COMMAND_OVERRIDES:
        return FILE_COMMAND_OVERRIDES[file_base]

    # Pattern-based overrides for external tool test families
    name = re.sub(r'^t\d+-', '', file_base)
    if name.startswith('git-svn') or name.startswith('svn'):
        return 'svn'
    if name.startswith('git-p4'):
        return 'p4'
    if name.startswith('git-cvsserver') or name.startswith('cvsserver'):
        return 'cvsserver'
    if name.startswith('cvsimport'):
        return 'cvsimport'
    if name.startswith('gitweb'):
        return 'gitweb'

    # Try progressively shorter prefixes of the name against known subcommands
    # e.g. "status-untracked-cache" → try "status-untracked-cache", "status-untracked", "status"
    parts = name.split('-')
    for i in range(len(parts), 0, -1):
        candidate = '-'.join(parts[:i])
        if candidate in GIT_SUBCOMMANDS:
            return candidate
    return None


def pick_best_subcommand(body, file_base):
    """Pick exactly one git subcommand for a test case.

    The file name is the strongest signal — t3200-branch.sh tests 'branch',
    even though many tests also call 'add', 'commit', etc. as setup.
    Only fall back to body scanning if the filename doesn't map to a command.
    """
    theme = file_theme_command(file_base)

    # If the filename maps to a known command, always use it.
    # This avoids attributing t3200-branch tests to 'add' just because
    # they run 'git add' during setup.
    if theme:
        return theme

    # No theme from filename — scan the body for commands
    found = extract_subcommands(body)
    if found:
        return found[0]

    # Last resort: assign to 'init' (general infrastructure)
    return "init"


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
                subcmd = pick_best_subcommand(body, base)
                out.write(f"{test_id}\t{base}\t{desc}\t{subcmd}\n")
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
    with ThreadPoolExecutor(max_workers=4) as pool:
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
