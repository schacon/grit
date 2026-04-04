#!/bin/sh
# Ported from git/t/t9116-git-svn-log.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn log tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'setup repository and import (requires SVN)' '
	false
'

test_expect_failure 'run log (requires SVN)' '
	false
'

test_expect_failure 'run log against a from trunk (requires SVN)' '
	false
'

test_expect_failure 'test ascending revision range (requires SVN)' '
	false
'

test_expect_failure 'test ascending revision range with --show-commit (requires SVN)' '
	false
'

test_expect_failure 'test ascending revision range with --show-commit (sha1) (requires SVN)' '
	false
'

test_expect_failure 'test descending revision range (requires SVN)' '
	false
'

test_expect_failure 'test ascending revision range with unreachable revision (requires SVN)' '
	false
'

test_expect_failure 'test descending revision range with unreachable revision (requires SVN)' '
	false
'

test_expect_failure 'test ascending revision range with unreachable upper boundary revision and 1 commit (requires SVN)' '
	false
'

test_expect_failure 'test descending revision range with unreachable upper boundary revision and 1 commit (requires SVN)' '
	false
'

test_expect_failure 'test ascending revision range with unreachable lower boundary revision and 1 commit (requires SVN)' '
	false
'

test_expect_failure 'test descending revision range with unreachable lower boundary revision and 1 commit (requires SVN)' '
	false
'

test_expect_failure 'test ascending revision range with unreachable boundary revisions and no commits (requires SVN)' '
	false
'

test_expect_failure 'test descending revision range with unreachable boundary revisions and no commits (requires SVN)' '
	false
'

test_expect_failure 'test ascending revision range with unreachable boundary revisions and 1 commit (requires SVN)' '
	false
'

test_expect_failure 'test descending revision range with unreachable boundary revisions and 1 commit (requires SVN)' '
	false
'

test_done
