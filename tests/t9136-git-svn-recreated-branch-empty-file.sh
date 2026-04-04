#!/bin/sh
#
# Upstream: t9136-git-svn-recreated-branch-empty-file.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='test recreated svn branch with empty files'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'load svn dumpfile' '
	false
'

test_expect_failure 'clone using git svn' '
	false
'

test_done
