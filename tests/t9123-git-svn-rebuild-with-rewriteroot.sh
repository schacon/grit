#!/bin/sh
# Ported from git/t/t9123-git-svn-rebuild-with-rewriteroot.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn respects rewriteRoot during rebuild'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'setup svn repository (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'init, fetch and checkout repository (requires SVN)' '
	false
'

test_expect_failure 'remove rev_map (requires SVN)' '
	false
'

test_expect_failure 'rebuild rev_map (requires SVN)' '
	false
'

test_done
