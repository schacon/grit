#!/bin/sh
#
# Upstream: t9141-git-svn-multiple-branches.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn multiple branch and tag paths in the svn repo'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
