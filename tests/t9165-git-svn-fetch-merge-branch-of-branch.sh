#!/bin/sh
#
# Upstream: t9165-git-svn-fetch-merge-branch-of-branch.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn merge detection'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize source svn repo' '
	false
'

test_expect_failure 'clone svn repo' '
	false
'

test_expect_failure 'verify merge commit' '
	false
'

test_done
