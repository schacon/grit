#!/bin/sh
#
# Upstream: t9150-svk-mergetickets.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git-svn svk merge tickets'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'load svk depot' '
	false
'

test_expect_failure 'svk merges were represented coming in' '
	false
'

test_done
