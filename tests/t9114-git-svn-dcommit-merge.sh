#!/bin/sh
#
# Upstream: t9114-git-svn-dcommit-merge.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn dcommit handles merges'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup svn repository' '
	false
'

test_expect_failure 'setup git mirror and merge' '
	false
'

test_expect_failure 'verify pre-merge ancestry' '
	false
'

test_expect_failure 'git svn dcommit merges' '
	false
'

test_expect_failure 'verify post-merge ancestry' '
	false
'

test_expect_failure 'verify merge commit message' '
	false
'

test_done
