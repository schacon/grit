#!/bin/sh
#
# Upstream: t9116-git-svn-log.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn log tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup repository and import' '
	false
'

test_expect_failure 'run log' '
	false
'

test_expect_failure 'run log against a from trunk' '
	false
'

test_expect_failure 'test ascending revision range' '
	false
'

test_expect_failure 'test ascending revision range with --show-commit' '
	false
'

test_expect_failure 'test ascending revision range with --show-commit (sha1)' '
	false
'

test_expect_failure 'test descending revision range' '
	false
'

test_expect_failure 'test ascending revision range with unreachable revision' '
	false
'

test_expect_failure 'test descending revision range with unreachable revision' '
	false
'

test_expect_failure 'test ascending revision range with unreachable upper boundary revision and 1 commit' '
	false
'

test_expect_failure 'test descending revision range with unreachable upper boundary revision and 1 commit' '
	false
'

test_expect_failure 'test ascending revision range with unreachable lower boundary revision and 1 commit' '
	false
'

test_expect_failure 'test descending revision range with unreachable lower boundary revision and 1 commit' '
	false
'

test_expect_failure 'test ascending revision range with unreachable boundary revisions and no commits' '
	false
'

test_expect_failure 'test descending revision range with unreachable boundary revisions and no commits' '
	false
'

test_expect_failure 'test ascending revision range with unreachable boundary revisions and 1 commit' '
	false
'

test_expect_failure 'test descending revision range with unreachable boundary revisions and 1 commit' '
	false
'

test_done
