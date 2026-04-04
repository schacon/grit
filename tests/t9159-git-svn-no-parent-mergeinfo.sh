#!/bin/sh
#
# Upstream: t9159-git-svn-no-parent-mergeinfo.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn handling of root commits in merge ranges'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
