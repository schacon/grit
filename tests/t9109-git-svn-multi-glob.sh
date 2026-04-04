#!/bin/sh
# Ported from git/t/t9109-git-svn-multi-glob.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn globbing refspecs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'test refspec globbing (requires SVN)' '
	false
'

test_expect_failure 'test left-hand-side only globbing (requires SVN)' '
	false
'

test_expect_failure 'test another branch (requires SVN)' '
	false
'

test_expect_failure 'prepare test disallow multiple globs (requires SVN)' '
	false
'

test_expect_failure 'test disallow multiple globs (requires SVN)' '
	false
'

test_done
