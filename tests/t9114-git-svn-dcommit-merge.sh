#!/bin/sh
# Ported from git/t/t9114-git-svn-dcommit-merge.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn dcommit handles merges'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'setup svn repository (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'setup git mirror and merge (requires SVN)' '
	false
'

test_expect_failure 'verify pre-merge ancestry (requires SVN)' '
	false
'

test_expect_failure 'git svn dcommit merges (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'verify post-merge ancestry (requires SVN)' '
	false
'

test_expect_failure 'verify merge commit message (requires SVN)' '
	false
'

test_done
