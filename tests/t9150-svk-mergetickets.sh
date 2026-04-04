#!/bin/sh
#
# Upstream: t9150-svk-mergetickets.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git-svn svk merge tickets'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
