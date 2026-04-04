#!/bin/sh
# Ported from git/t/t9103-git-svn-tracked-directory-removed.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn tracking removed top-level path'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'make history for tracking (requires SVN)' '
	false
'

test_expect_failure 'clone repo with git (requires SVN)' '
	false
'

test_expect_failure 'make sure r2 still has old file (requires SVN)' '
	false
'

test_done
