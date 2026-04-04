#!/bin/sh
#
# Upstream: t9151-svn-mergeinfo.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git-svn svn mergeinfo properties'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'load svn dump' '
	false
'

test_expect_failure 'all svn merges became git merge commits' '
	false
'

test_expect_failure 'cherry picks did not become git merge commits' '
	false
'

test_expect_failure 'svn non-merge merge commits did not become git merge commits' '
	false
'

test_expect_failure 'commit made to merged branch is reachable from the merge' '
	false
'

test_expect_failure 'merging two branches in one commit is detected correctly' '
	false
'

test_expect_failure 'everything got merged in the end' '
	false
'

test_done
