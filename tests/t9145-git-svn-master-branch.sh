#!/bin/sh
#
# Upstream: t9145-git-svn-master-branch.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn initial main branch is "trunk" if possible'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
