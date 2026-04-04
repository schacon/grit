#!/bin/sh
#
# Upstream: t9118-git-svn-funky-branch-names.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn funky branch names'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
