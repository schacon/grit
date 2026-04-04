#!/bin/sh
#
# Upstream: t9164-git-svn-dcommit-concurrent.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='concurrent git svn dcommit'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup svn repository' '
	false
'

test_expect_failure 'check if post-commit hook creates a concurrent commit' '
	false
'

test_expect_failure 'check if pre-commit hook fails' '
	false
'

test_expect_failure 'dcommit error handling' '
	false
'

test_expect_failure 'dcommit concurrent change in non-changed file' '
	false
'

test_expect_failure 'dcommit concurrent non-conflicting change' '
	false
'

test_expect_failure 'dcommit --no-rebase concurrent non-conflicting change' '
	false
'

test_expect_failure 'dcommit fails on concurrent conflicting change' '
	false
'

test_done
