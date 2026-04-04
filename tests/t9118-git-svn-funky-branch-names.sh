#!/bin/sh
# Ported from git/t/t9118-git-svn-funky-branch-names.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn funky branch names'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'setup svnrepo (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'test clone with funky branch names (requires SVN)' '
	false
'

test_expect_failure 'test dcommit to funky branch (requires SVN)' '
	false
'

test_expect_failure 'test dcommit to scary branch (requires SVN)' '
	false
'

test_expect_failure 'test dcommit to trailing_dotlock branch (requires SVN)' '
	false
'

test_done
