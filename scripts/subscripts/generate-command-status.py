#!/usr/bin/env python3
"""
Script 2: Generate command status from test results.

Reads:
  data/test-results.tsv
  data/file-results.tsv

Produces:
  data/command-status.tsv — one row per git subcommand with:
    command, category, started, total_tests, passing_tests, pct
"""

import os
import re
from collections import defaultdict

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DATA = os.path.join(REPO, "data")
COMMANDS_DIR = os.path.join(REPO, "grit", "src", "commands")

# ── Canonical git subcommand list with categories ──

CATEGORIES = {
    "Main Porcelain": [
        "add", "am", "archive", "bisect", "branch", "checkout", "cherry-pick",
        "clean", "clone", "commit", "describe", "diff", "fetch", "format-patch",
        "gc", "grep", "init", "log", "merge", "mv", "notes", "pull", "push",
        "range-diff", "rebase", "reset", "restore", "revert", "rm", "shortlog",
        "show", "stash", "status", "submodule", "switch", "tag", "worktree",
    ],
    "Ancillary Porcelain": [
        "annotate", "blame", "bundle", "cherry", "config", "count-objects",
        "diagnose", "difftool", "fast-export", "fast-import", "filter-branch",
        "fsck", "help", "maintenance", "merge-tree", "mergetool", "pack-refs",
        "prune", "reflog", "remote", "repack", "replace", "request-pull",
        "rerere", "scalar", "show-branch", "sparse-checkout", "verify-commit",
        "verify-tag", "version", "whatchanged",
    ],
    "Plumbing": [
        "apply", "cat-file", "check-attr", "check-ignore", "check-mailmap",
        "check-ref-format", "checkout-index", "column", "commit-graph",
        "commit-tree", "credential", "credential-cache", "credential-store",
        "daemon", "diff-files", "diff-index", "diff-tree", "fetch-pack",
        "fmt-merge-msg", "for-each-ref", "for-each-repo", "get-tar-commit-id",
        "hash-object", "hook", "http-backend", "index-pack",
        "interpret-trailers", "ls-files", "ls-remote", "ls-tree", "mailinfo",
        "mailsplit", "merge-base", "merge-file", "merge-index",
        "merge-one-file", "mktag", "mktree", "multi-pack-index", "name-rev",
        "pack-objects", "pack-redundant", "patch-id", "prune-packed",
        "read-tree", "receive-pack", "refs", "rev-list", "rev-parse",
        "send-pack", "show-index", "show-ref", "stripspace", "symbolic-ref",
        "unpack-file", "unpack-objects", "update-index", "update-ref",
        "update-server-info", "upload-archive", "upload-pack", "var",
        "verify-pack", "write-tree",
    ],
}

CMD_TO_CATEGORY = {}
ALL_COMMANDS = []
for cat, cmds in CATEGORIES.items():
    for cmd in cmds:
        CMD_TO_CATEGORY[cmd] = cat
        ALL_COMMANDS.append(cmd)


def command_file_name(cmd):
    return cmd.replace('-', '_') + '.rs'


def is_started(cmd):
    fname = command_file_name(cmd)
    return os.path.isfile(os.path.join(COMMANDS_DIR, fname))


def load_file_pass_rates():
    """Load file-results.tsv → { file_base: pass_rate (0.0-1.0) }"""
    path = os.path.join(DATA, "file-results.tsv")
    rates = {}
    with open(path) as f:
        f.readline()  # header
        for line in f:
            parts = line.strip().split('\t')
            if len(parts) < 6:
                continue
            fbase = parts[0]
            ported = parts[1] == 'yes'
            total = int(parts[2])
            passing = int(parts[3])
            if ported and total > 0:
                rates[fbase] = passing / total
            elif ported:
                rates[fbase] = 0.0
    return rates


def load_test_results(file_pass_rates):
    """Load test-results.tsv and aggregate by subcommand.

    For each test case, use the file's pass rate to proportionally count passing.
    """
    results_path = os.path.join(DATA, "test-results.tsv")
    stats = defaultdict(lambda: {'total': 0, 'passing': 0.0, 'not_ported': 0})

    with open(results_path) as f:
        f.readline()  # header
        for line in f:
            parts = line.rstrip('\n').split('\t')
            if len(parts) < 5:
                continue
            fbase = parts[1]
            subcmds_str = parts[3]
            status = parts[4]
            if not subcmds_str:
                continue
            subcmds = [c.strip() for c in subcmds_str.split(',') if c.strip()]
            for cmd in subcmds:
                stats[cmd]['total'] += 1
                if status == 'pass':
                    stats[cmd]['passing'] += 1.0
                elif status == 'partial':
                    # Use proportional pass rate for the file
                    rate = file_pass_rates.get(fbase, 0.0)
                    stats[cmd]['passing'] += rate
                else:
                    stats[cmd]['not_ported'] += 1

    return stats


def main():
    file_pass_rates = load_file_pass_rates()
    stats = load_test_results(file_pass_rates)
    outpath = os.path.join(DATA, "command-status.tsv")

    with open(outpath, 'w') as out:
        out.write("command\tcategory\tstarted\ttotal_tests\tpassing_tests\tnot_ported\tpct_passing\n")
        for cmd in ALL_COMMANDS:
            cat = CMD_TO_CATEGORY[cmd]
            started = "yes" if is_started(cmd) else "no"
            s = stats.get(cmd, {'total': 0, 'passing': 0.0, 'not_ported': 0})
            total = s['total']
            passing = round(s['passing'])
            not_ported = s['not_ported']
            pct = round(100 * s['passing'] / total, 1) if total > 0 else 0.0
            out.write(f"{cmd}\t{cat}\t{started}\t{total}\t{passing}\t{not_ported}\t{pct}\n")

    started_count = sum(1 for cmd in ALL_COMMANDS if is_started(cmd))
    total_cmds = len(ALL_COMMANDS)
    print(f"Commands: {started_count}/{total_cmds} started ({100*started_count//total_cmds}%)")
    print(f"Wrote {outpath}")


if __name__ == '__main__':
    main()
