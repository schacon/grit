#!/bin/sh
# Ported from git/t/t9102-git-svn-deep-rmdir.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn rmdir'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'initialize repo (requires SVN)' '
	false
'

test_expect_failure 'mirror via git svn (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'Try a commit on rmdir (requires SVN)' '
	false
'

test_done
