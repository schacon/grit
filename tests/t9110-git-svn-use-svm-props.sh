#!/bin/sh
# Ported from git/t/t9110-git-svn-use-svm-props.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn useSvmProps test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'load svm repo (requires SVN)' '
	false
'

test_expect_failure 'verify metadata for /bar (requires SVN)' '
	false
'

test_expect_failure 'verify metadata for /dir/a/b/c/d/e (requires SVN)' '
	false
'

test_expect_failure 'verify metadata for /dir (requires SVN)' '
	false
'

test_expect_failure 'find commit based on SVN revision number (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'empty rebase (requires SVN)' '
	false
'

test_done
