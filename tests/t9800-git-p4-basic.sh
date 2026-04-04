#!/bin/sh
#
# Upstream: t9800-git-p4-basic.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'add p4 files' '
	false
'

test_expect_failure 'basic git p4 clone' '
	false
'

test_expect_failure 'depot typo error' '
	false
'

test_expect_failure 'git p4 clone @all' '
	false
'

test_expect_failure 'git p4 sync uninitialized repo' '
	false
'

test_expect_failure 'git p4 sync new branch' '
	false
'

test_expect_failure 'git p4 sync existing branch without changes' '
	false
'

test_expect_failure 'git p4 sync existing branch with relative name' '
	false
'

test_expect_failure 'git p4 sync existing branch with nested path' '
	false
'

test_expect_failure 'git p4 sync branch explicit ref without p4 in path' '
	false
'

test_expect_failure 'git p4 sync nonexistent ref' '
	false
'

test_expect_failure 'git p4 sync existing non-p4-imported ref' '
	false
'

test_expect_failure 'clone two dirs' '
	false
'

test_expect_failure 'clone two dirs, @all' '
	false
'

test_expect_failure 'clone two dirs, @all, conflicting files' '
	false
'

test_expect_failure 'clone two dirs, each edited by submit, single git commit' '
	false
'

test_expect_failure 'clone using non-numeric revision ranges' '
	false
'

test_expect_failure 'clone with date range, excluding some changes' '
	false
'

test_expect_failure 'exit when p4 fails to produce marshaled output' '
	false
'

test_expect_failure 'exit gracefully for p4 server errors' '
	false
'

test_expect_failure 'clone --bare should make a bare repository' '
	false
'

test_expect_failure 'initial import time from top change time' '
	false
'

test_expect_failure 'unresolvable host in P4PORT should display error' '
	false
'

test_expect_failure 'run hook p4-pre-submit before submit' '
	false
'

test_expect_failure 'submit from detached head' '
	false
'

test_expect_failure 'submit from worktree' '
	false
'

test_done
