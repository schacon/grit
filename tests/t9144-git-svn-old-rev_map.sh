#!/bin/sh
#
# Upstream: t9144-git-svn-old-rev_map.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn old rev_map preservd'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup test repository with old layout' '
	false
'

test_expect_failure 'old layout continues to work' '
	false
'

test_done
