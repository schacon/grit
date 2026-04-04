#!/bin/sh
# Ported from git/t/t9105-git-svn-commit-diff.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn commit-diff'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'initialize repo (requires SVN)' '
	false
'

test_expect_failure 'test the commit-diff command (requires SVN)' '
	false
'

test_expect_failure 'commit-diff to a sub-directory (with git svn config) (not ported - requires SVN infrastructure)' '
	false
'

test_done
