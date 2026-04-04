#!/bin/sh
#
# Upstream: t9161-git-svn-mergeinfo-push.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git-svn svn mergeinfo propagation'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'load svn dump' '
	false
'

test_expect_failure 'propagate merge information' '
	false
'

test_expect_failure 'check svn:mergeinfo' '
	false
'

test_expect_failure 'merge another branch' '
	false
'

test_expect_failure 'check primary parent mergeinfo respected' '
	false
'

test_expect_failure 'merge existing merge' '
	false
'

test_expect_failure 'make further commits to branch' '
	false
'

test_expect_failure 'second forward merge' '
	false
'

test_expect_failure 'check new mergeinfo added' '
	false
'

test_expect_failure 'reintegration merge' '
	false
'

test_expect_failure 'check reintegration mergeinfo' '
	false
'

test_expect_failure 'dcommit a merge at the top of a stack' '
	false
'

test_expect_failure 'check both parents'\'' mergeinfo respected' '
	false
'

test_done
