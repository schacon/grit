#!/bin/sh
# Ported from git/t/t9122-git-svn-author.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn authorship'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'setup svn repository (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'interact with it via git svn (not ported - requires SVN infrastructure)' '
	false
'

test_done
