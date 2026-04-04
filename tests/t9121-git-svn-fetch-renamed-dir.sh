#!/bin/sh
# Ported from git/t/t9121-git-svn-fetch-renamed-dir.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn can fetch renamed directories'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'load repository with renamed directory (requires SVN)' '
	false
'

test_expect_failure 'init and fetch repository (requires SVN)' '
	false
'

test_done
