#!/bin/sh
# Ported from git/t/t9127-git-svn-partial-rebuild.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn partial-rebuild tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'initialize svnrepo (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'import an early SVN revision into git (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'make full git mirror of SVN (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'fetch from git mirror and partial-rebuild (requires SVN)' '
	false
'

test_done
