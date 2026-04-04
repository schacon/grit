#!/bin/sh
#
# Upstream: t9105-git-svn-commit-diff.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn commit-diff'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize repo' '
	false
'

test_expect_failure 'test the commit-diff command' '
	false
'

test_expect_failure 'commit-diff to a sub-directory (with git svn config)' '
	false
'

test_done
