#!/bin/sh
#
# Upstream: t9128-git-svn-cmd-branch.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn partial-rebuild tests'

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

test_expect_failure 'branch uses correct svn-remote' '
	false
'

test_done
