#!/bin/sh
#
# Upstream: t9141-git-svn-multiple-branches.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn multiple branch and tag paths in the svn repo'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup svnrepo' '
	false
'

test_expect_failure 'clone multiple branch and tag paths' '
	false
'

test_expect_failure 'Multiple branch or tag paths require -d' '
	false
'

test_expect_failure 'create new branches and tags' '
	false
'

test_done
