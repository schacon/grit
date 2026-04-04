#!/bin/sh
#
# Upstream: t9135-git-svn-moved-branch-empty-file.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='test moved svn branch with missing empty files'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'load svn dumpfile' '
	false
'

test_expect_failure 'clone using git svn' '
	false
'

test_expect_failure 'test that b1 exists and is empty' '
	false
'

test_done
