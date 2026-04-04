#!/bin/sh
#
# Upstream: t9123-git-svn-rebuild-with-rewriteroot.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn respects rewriteRoot during rebuild'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
