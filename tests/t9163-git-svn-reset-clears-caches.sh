#!/bin/sh
#
# Upstream: t9163-git-svn-reset-clears-caches.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn reset clears memoized caches'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize source svn repo' '
	false
'

test_expect_failure 'fetch to merge-base (a)' '
	false
'

test_expect_failure 'rebase looses SVN merge (m)' '
	false
'

test_expect_failure 'reset and fetch gets the SVN merge (m) correctly' '
	false
'

test_done
