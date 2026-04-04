#!/bin/sh
# Ported from git/t/t9106-git-svn-commit-diff-clobber.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn commit-diff clobber'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'initialize repo (requires SVN)' '
	false
'

test_expect_failure 'commit change from svn side (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'commit conflicting change from git (requires SVN)' '
	false
'

test_expect_failure 'commit complementing change from git (requires SVN)' '
	false
'

test_expect_failure 'dcommit fails to commit because of conflict (requires SVN)' '
	false
'

test_expect_failure 'dcommit does the svn equivalent of an index merge (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'commit another change from svn side (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'multiple dcommit from git svn will not clobber svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'check that rebase really failed (requires SVN)' '
	false
'

test_expect_failure 'resolve, continue the rebase and dcommit (requires SVN)' '
	false
'

test_done
