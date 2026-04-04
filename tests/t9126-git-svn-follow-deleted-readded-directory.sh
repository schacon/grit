#!/bin/sh
# Ported from git/t/t9126-git-svn-follow-deleted-readded-directory.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn fetch repository with deleted and readded directory'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'load repository (requires SVN)' '
	false
'

test_expect_failure 'fetch repository (requires SVN)' '
	false
'

test_done
