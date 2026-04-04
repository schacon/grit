#!/bin/sh
# Ported from git/t/t9113-git-svn-dcommit-new-file.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn dcommit new files over svn:// test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'start tracking an empty repo (requires SVN)' '
	false
'

test_expect_failure 'create files in new directory with dcommit (requires SVN)' '
	false
'

test_done
