#!/bin/sh
# Ported from git/t/t9111-git-svn-use-svnsync-props.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn useSvnsyncProps test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'load svnsync repo (not ported - requires SVN infrastructure)' '
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

test_done
