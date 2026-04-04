#!/bin/sh
# Ported from git/t/t9133-git-svn-nested-git-repo.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn property tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'setup repo with a git repo inside it (requires SVN)' '
	false
'

test_expect_failure 'clone an SVN repo containing a git repo (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'SVN-side change outside of .git (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'update git svn-cloned repo (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'SVN-side change inside of .git (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'update git svn-cloned repo (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'SVN-side change in and out of .git (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'update git svn-cloned repo again (not ported - requires SVN infrastructure)' '
	false
'

test_done
