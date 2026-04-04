#!/bin/sh
#
# Upstream: t9123-git-svn-rebuild-with-rewriteroot.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn respects rewriteRoot during rebuild'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup svn repository' '
	false
'

test_expect_failure 'init, fetch and checkout repository' '
	false
'

test_expect_failure 'remove rev_map' '
	false
'

test_expect_failure 'rebuild rev_map' '
	false
'

test_done
