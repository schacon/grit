#!/bin/sh
#
# Upstream: t9106-git-svn-commit-diff-clobber.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn commit-diff clobber'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize repo' '
	false
'

test_expect_failure 'commit change from svn side' '
	false
'

test_expect_failure 'commit conflicting change from git' '
	false
'

test_expect_failure 'commit complementing change from git' '
	false
'

test_expect_failure 'dcommit fails to commit because of conflict' '
	false
'

test_expect_failure 'dcommit does the svn equivalent of an index merge' '
	false
'

test_expect_failure 'commit another change from svn side' '
	false
'

test_expect_failure 'multiple dcommit from git svn will not clobber svn' '
	false
'

test_expect_failure 'check that rebase really failed' '
	false
'

test_expect_failure 'resolve, continue the rebase and dcommit' '
	false
'

test_done
