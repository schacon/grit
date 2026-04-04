#!/bin/sh
#
# Upstream: t9161-git-svn-mergeinfo-push.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git-svn svn mergeinfo propagation'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
