#!/bin/sh
#
# Upstream: t9140-git-svn-reset.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn reset'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup test repository' '
	false
'

test_expect_failure 'clone SVN repository with hidden directory' '
	false
'

test_expect_failure 'modify hidden file in SVN repo' '
	false
'

test_expect_failure 'fetch fails on modified hidden file' '
	false
'

test_expect_failure 'reset unwinds back to r1' '
	false
'

test_expect_failure 'refetch succeeds not ignoring any files' '
	false
'

test_done
