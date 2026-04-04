#!/bin/sh
# Ported from git/t/t9128-git-svn-cmd-branch.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn partial-rebuild tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'initialize svnrepo (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'import into git (requires SVN)' '
	false
'

test_expect_failure 'git svn branch tests (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'branch uses correct svn-remote (not ported - requires SVN infrastructure)' '
	false
'

test_done
