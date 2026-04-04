#!/bin/sh
#
# Upstream: t9163-git-svn-reset-clears-caches.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn reset clears memoized caches'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
