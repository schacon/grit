#!/bin/sh
# Ported from git/t/t9112-git-svn-md5less-file.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='test that git handles an svn repository with missing md5sums'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'load svn dumpfile (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'initialize git svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'fetch revisions from svn (not ported - requires SVN infrastructure)' '
	false
'

test_done
