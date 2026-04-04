#!/bin/sh
#
# Upstream: t9152-svn-empty-dirs-after-gc.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn creates empty directories, calls git gc, makes sure they are still empty'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
