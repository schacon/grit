#!/bin/sh
#
# Upstream: t9167-git-svn-cmd-branch-subproject.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn branch for subproject clones'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize svnrepo' '
	false
'

test_expect_failure 'import into git' '
	false
'

test_expect_failure 'git svn branch tests' '
	false
'

test_done
